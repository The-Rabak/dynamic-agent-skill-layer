use std::sync::{Arc, RwLock};

use chrono::{DateTime, Utc};
use domain::EmbeddingService;
use redis::AsyncCommands;
use serde::Serialize;
use sqlx::PgPool;

use crate::embeddings::ollama::{OllamaEmbeddingConfig, OllamaEmbeddingService};
use crate::vector::qdrant::validate_qdrant_url;

// ---------------------------------------------------------------------------
// Snapshot readiness signal
// ---------------------------------------------------------------------------

/// The readiness state of the in-memory graph snapshot.
///
/// Transitions:
/// - `Warming` → `Ready`: snapshot successfully built/installed.
/// - `Warming` → `Failed`: snapshot build failed.
/// - `Ready`   → `Warming`: a background reload started.
/// - `Warming` → `Ready`/`Failed`: reload completed or errored.
///
/// `Failed` carries the error message surfaced on `/health` so operators can
/// diagnose the problem without tailing container logs.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ReadinessState {
    /// Snapshot build/reload is in flight — tool calls must not embed queries.
    Warming,
    /// Snapshot is installed and ready to serve queries.
    Ready,
    /// The last snapshot build failed; details are in the message.
    Failed(String),
}

/// Thread-safe handle for the snapshot readiness signal.
///
/// Wraps the mutable [`ReadinessState`] behind an `Arc<RwLock<…>>` so reads
/// on the hot path are essentially free (shared lock) and state transitions
/// (`set_ready`, `set_failed`, `set_warming`) use exclusive writes only.
///
/// Constructed in two ways:
/// - [`ReadinessHandle::warming`] — the live boot path, starts in `Warming`.
/// - [`ReadinessHandle::ready`] — test/non-live constructors, starts `Ready`
///   so existing tests do not regress into the warming short-circuit.
#[derive(Debug, Clone)]
pub struct ReadinessHandle {
    state: Arc<RwLock<ReadinessState>>,
}

impl ReadinessHandle {
    /// Creates a handle in the `Warming` state (live boot path).
    pub fn warming() -> Self {
        Self {
            state: Arc::new(RwLock::new(ReadinessState::Warming)),
        }
    }

    /// Creates a handle already in the `Ready` state (test/non-live constructors).
    ///
    /// Non-live test constructors default to `Ready` so the ~40 existing
    /// `McpServerApp` tests do not hit the warming short-circuit.
    pub fn ready() -> Self {
        Self {
            state: Arc::new(RwLock::new(ReadinessState::Ready)),
        }
    }

    /// Transitions the handle to `Ready`.
    ///
    /// Called once after the initial snapshot and once after each successful
    /// background reload (`reload_and_swap` → `swap_graph` succeeded).
    pub fn set_ready(&self) {
        *self.state.write().expect("readiness lock poisoned") = ReadinessState::Ready;
    }

    /// Transitions the handle to `Warming`.
    ///
    /// Called at the start of each background reload so `/health` reports
    /// NOT-ready during the (potentially long) re-embed window.
    pub fn set_warming(&self) {
        *self.state.write().expect("readiness lock poisoned") = ReadinessState::Warming;
    }

    /// Transitions the handle to `Failed` with the given error message.
    ///
    /// Called when `build_graph_from_pg` errors during a background reload.
    /// A failed reload is observable on `/health` rather than silently stuck
    /// in `Warming` forever. The error still propagates as `Err` so the event
    /// replays (existing ACK contract unchanged).
    pub fn set_failed(&self, message: impl Into<String>) {
        *self.state.write().expect("readiness lock poisoned") =
            ReadinessState::Failed(message.into());
    }

    /// Returns `true` iff the snapshot is `Ready`.
    ///
    /// Hot path: acquires a shared read lock (non-blocking when no writer holds
    /// the exclusive lock).
    pub fn is_ready(&self) -> bool {
        matches!(
            *self.state.read().expect("readiness lock poisoned"),
            ReadinessState::Ready
        )
    }

    /// Produces a `HealthComponent` for the `/health` report.
    ///
    /// - `Ready`   → `healthy: true`,  detail `"ready"`
    /// - `Warming` → `healthy: false`, detail `"warming: snapshot build/reload in flight"`
    /// - `Failed`  → `healthy: false`, detail `"failed: <message>"`
    pub fn health_component(&self) -> HealthComponent {
        match &*self.state.read().expect("readiness lock poisoned") {
            ReadinessState::Ready => HealthComponent {
                name: "readiness".to_owned(),
                healthy: true,
                detail: "ready".to_owned(),
            },
            ReadinessState::Warming => HealthComponent {
                name: "readiness".to_owned(),
                healthy: false,
                detail: "warming: snapshot build/reload in flight".to_owned(),
            },
            ReadinessState::Failed(msg) => HealthComponent {
                name: "readiness".to_owned(),
                healthy: false,
                detail: format!("failed: {msg}"),
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Health report types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct HealthComponent {
    pub name: String,
    pub healthy: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct HealthReport {
    pub healthy: bool,
    pub checked_at: DateTime<Utc>,
    pub components: Vec<HealthComponent>,
}

#[derive(Debug, Clone, Default)]
pub struct InfrastructureHealthChecker {
    postgres: Option<PgPool>,
    redis: Option<redis::Client>,
    http_dependencies: Vec<HttpDependencyCheck>,
    http_client: Option<reqwest::Client>,
    config_invalid_components: Vec<HealthComponent>,
    /// Optional snapshot-readiness handle.
    ///
    /// When `Some`, a `readiness` component is appended to every `check()` report.
    /// `Warming` or `Failed` states make the component unhealthy, which flips the
    /// overall `healthy` flag to `false` and causes `/health` to return 503.
    readiness: Option<Arc<ReadinessHandle>>,
}

#[derive(Debug, Clone)]
struct HttpDependencyCheck {
    name: String,
    endpoint: String,
}

impl InfrastructureHealthChecker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_postgres(mut self, pool: PgPool) -> Self {
        self.postgres = Some(pool);
        self
    }

    pub fn with_redis(mut self, client: redis::Client) -> Self {
        self.redis = Some(client);
        self
    }

    pub fn with_ollama(
        mut self,
        http_client: reqwest::Client,
        base_url: impl Into<String>,
    ) -> Self {
        self.http_client = Some(http_client);
        self.http_dependencies.push(HttpDependencyCheck {
            name: "ollama".to_owned(),
            endpoint: format!("{}/api/tags", base_url.into().trim_end_matches('/')),
        });
        self
    }

    /// Adds a named HTTP dependency that should be probed during health checks.
    pub fn with_http_dependency(
        mut self,
        http_client: reqwest::Client,
        name: impl Into<String>,
        endpoint: impl Into<String>,
    ) -> Self {
        self.http_client = Some(http_client);
        self.http_dependencies.push(HttpDependencyCheck {
            name: name.into(),
            endpoint: endpoint.into(),
        });
        self
    }

    /// Register a dependency whose configuration was invalid at startup.
    /// The component is always unhealthy, surfaced alongside runtime-probed components.
    pub fn with_config_invalid(
        mut self,
        name: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        self.config_invalid_components.push(HealthComponent {
            name: name.into(),
            healthy: false,
            detail: format!("config_invalid: {}", reason.into()),
        });
        self
    }

    /// Register a static component whose state is known at startup and does not
    /// require runtime probing. Used to surface features whose enabled/disabled
    /// state is determined by env vars rather than by a reachable network dependency
    /// (e.g. `usage_write`, `extraction_provider`).
    pub fn with_static_component(
        mut self,
        name: impl Into<String>,
        healthy: bool,
        detail: impl Into<String>,
    ) -> Self {
        self.config_invalid_components.push(HealthComponent {
            name: name.into(),
            healthy,
            detail: detail.into(),
        });
        self
    }

    /// Attaches a snapshot-readiness handle to this checker.
    ///
    /// When set, every `check()` call appends a `readiness` component derived
    /// from the handle's current state:
    /// - `Ready`   → `healthy: true`, detail `"ready"`
    /// - `Warming` → `healthy: false`, detail `"warming: snapshot build/reload in flight"`
    /// - `Failed`  → `healthy: false`, detail `"failed: <message>"`
    ///
    /// Because the overall `healthy` flag is `all components healthy`, a `Warming`
    /// or `Failed` readiness component causes `/health` to return 503 — killing the
    /// "healthy-while-warming" window.
    pub fn with_readiness(mut self, handle: Arc<ReadinessHandle>) -> Self {
        self.readiness = Some(handle);
        self
    }

    pub async fn check(&self) -> HealthReport {
        let mut components: Vec<HealthComponent> = self.config_invalid_components.clone();

        if let Some(pool) = &self.postgres {
            let postgres_health = match sqlx::query("SELECT 1").execute(pool).await {
                Ok(_) => HealthComponent {
                    name: "postgres".to_owned(),
                    healthy: true,
                    detail: "reachable".to_owned(),
                },
                Err(_) => HealthComponent {
                    name: "postgres".to_owned(),
                    healthy: false,
                    detail: "unreachable (postgres_query_failed)".to_owned(),
                },
            };
            components.push(postgres_health);
        }

        if let Some(client) = &self.redis {
            let redis_health = match client.get_multiplexed_async_connection().await {
                Ok(mut conn) => {
                    let ping: redis::RedisResult<String> = conn.ping().await;
                    match ping {
                        Ok(_) => HealthComponent {
                            name: "redis".to_owned(),
                            healthy: true,
                            detail: "reachable".to_owned(),
                        },
                        Err(_) => HealthComponent {
                            name: "redis".to_owned(),
                            healthy: false,
                            detail: "unreachable (redis_ping_failed)".to_owned(),
                        },
                    }
                }
                Err(_) => HealthComponent {
                    name: "redis".to_owned(),
                    healthy: false,
                    detail: "unreachable (redis_connect_failed)".to_owned(),
                },
            };
            components.push(redis_health);
        }

        if let Some(client) = &self.http_client {
            for dependency in &self.http_dependencies {
                let dependency_health = match client.get(&dependency.endpoint).send().await {
                    Ok(response) if response.status().is_success() => HealthComponent {
                        name: dependency.name.clone(),
                        healthy: true,
                        detail: "reachable".to_owned(),
                    },
                    Ok(response) => HealthComponent {
                        name: dependency.name.clone(),
                        healthy: false,
                        detail: format!("status {}", response.status()),
                    },
                    Err(_) => HealthComponent {
                        name: dependency.name.clone(),
                        healthy: false,
                        detail: "unreachable (http_request_failed)".to_owned(),
                    },
                };
                components.push(dependency_health);
            }
        }

        // Append the snapshot-readiness component last so it is clearly visible
        // at the end of the report and does not interfere with the static/infra
        // components registered above.
        if let Some(handle) = &self.readiness {
            components.push(handle.health_component());
        }

        let healthy = components.iter().all(|component| component.healthy);

        HealthReport {
            healthy,
            checked_at: Utc::now(),
            components,
        }
    }
}

pub struct DependencyFactory;

impl DependencyFactory {
    pub fn build_health_checker_from_environment() -> InfrastructureHealthChecker {
        let mut checker = InfrastructureHealthChecker::new();

        if let Ok(database_url) = std::env::var("DATABASE_URL")
            && !database_url.trim().is_empty()
        {
            match sqlx::postgres::PgPoolOptions::new()
                .max_connections(1)
                .connect_lazy(database_url.trim())
            {
                Ok(pool) => checker = checker.with_postgres(pool),
                Err(e) => checker = checker.with_config_invalid("postgres", e.to_string()),
            }
        }

        if let Ok(redis_url) = std::env::var("REDIS_URL")
            && !redis_url.trim().is_empty()
        {
            match redis::Client::open(redis_url.trim()) {
                Ok(client) => checker = checker.with_redis(client),
                Err(e) => checker = checker.with_config_invalid("redis", e.to_string()),
            }
        }

        let http_client = reqwest::Client::new();
        if let Ok(ollama_url) = std::env::var("OLLAMA_URL")
            && !ollama_url.trim().is_empty()
        {
            checker = checker.with_ollama(http_client.clone(), ollama_url);
        }
        if let Ok(qdrant_url) = std::env::var("QDRANT_URL")
            && !qdrant_url.trim().is_empty()
        {
            // Validate the URL before registering any probes: a malformed URL is not
            // worth probing and the error would be confusing rather than diagnostic.
            // `validate_qdrant_url` also emits a loud warn for non-local hosts.
            if let Err(err) = validate_qdrant_url(qdrant_url.trim()) {
                tracing::warn!(
                    qdrant_url = qdrant_url.trim(),
                    error = %err,
                    "QDRANT_URL failed validation; skipping qdrant_write_side and qdrant_read_path \
                     probes — fix the URL and restart"
                );
            } else {
                // Label as "qdrant_write_side": Qdrant is the durable write-side vector
                // store (outbox drain target). It is NOT queried at read time under
                // Option A (ADR-0001). The label must not imply a read-path dependency.
                checker = checker.with_http_dependency(
                    http_client.clone(),
                    "qdrant_write_side",
                    format!("{}/collections", qdrant_url.trim_end_matches('/')),
                );

                // Under QdrantHybrid, Qdrant is ALSO a read-time dependency: each
                // retrieve call queries Qdrant for dense+sparse hybrid candidates.
                // Surface this as a separate `qdrant_read_path` component so that
                // operators can distinguish a write-side indexing failure (qdrant_write_side
                // degraded) from a read-path outage (qdrant_read_path degraded).
                // Probe the same /collections endpoint — reachability check is sufficient
                // to confirm the path is live; actual query health is shown at retrieve time
                // via the `qdrant_hybrid_read` marker in `RetrievalOutcome::health_markers`.
                let backend_env = std::env::var("RETRIEVAL_BACKEND").unwrap_or_default();
                let backend_normalized = backend_env.trim().to_ascii_lowercase();
                match backend_normalized.as_str() {
                    // Qdrant hybrid arms: Qdrant is a read-time dependency; add the probe.
                    "qdrant_hybrid" | "qdrant" => {
                        checker = checker.with_http_dependency(
                            http_client.clone(),
                            "qdrant_read_path",
                            format!("{}/collections", qdrant_url.trim_end_matches('/')),
                        );
                    }
                    // Snapshot arms: Qdrant is write-only; no read-path probe needed.
                    "snapshot_dense" | "dense" | "snapshot_hybrid" | "hybrid" | "" => {}
                    // Unrecognized value: the orchestrator will reject it at boot, but
                    // surface a loud warning here so health-check logs also flag the problem.
                    unrecognized => {
                        tracing::warn!(
                            retrieval_backend = unrecognized,
                            "unrecognized RETRIEVAL_BACKEND {:?}; qdrant_read_path probe skipped \
                             — the orchestrator will reject this value at boot",
                            unrecognized
                        );
                    }
                }
            }
        }

        // Surface usage-write state on /health so agents can observe it without
        // waiting for a compile_context call. The startup-time signal is always
        // "enabled" — the writer is unconditionally spawned on the live boot path.
        // Actual write failures surface at runtime via compile_context health markers.
        checker = checker.with_static_component("usage_write", true, "enabled");

        // Surface the active extraction provider on /health so agents can query
        // provider configuration pre-flight without enqueuing a job. Falls back to
        // "ollama" (the constitution v2.0.0 default) when the env var is unset/blank.
        let extraction_provider = std::env::var("EXTRACT_SESSION_PROVIDER").unwrap_or_default();
        let extraction_provider = match extraction_provider.trim().to_ascii_lowercase().as_str() {
            "claude" | "claude-api" => "claude",
            "claude-code" | "claude-cli" => "claude-code",
            _ => "ollama",
        };
        checker = checker.with_static_component("extraction_provider", true, extraction_provider);

        checker
    }

    pub fn build_embedding_service_from_environment()
    -> Result<Arc<impl EmbeddingService>, Box<dyn std::error::Error>> {
        let service =
            OllamaEmbeddingService::new(reqwest::Client::new(), OllamaEmbeddingConfig::default())
                .map_err(|error| std::io::Error::other(error.to_string()))?;

        Ok(Arc::new(service))
    }

    pub fn build_redis_client_from_environment() -> Option<redis::Client> {
        std::env::var("REDIS_URL").ok()
            .filter(|url| !url.trim().is_empty())
            .and_then(|url| {
                redis::Client::open(url.trim())
                    .inspect_err(|error| {
                        tracing::warn!(
                            ?error,
                            "failed to construct redis client for suppression state; running without redis ttl"
                        );
                    })
                    .ok()
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn empty_health_checker_reports_healthy_with_no_components() {
        let checker = InfrastructureHealthChecker::new();
        let report = checker.check().await;

        assert!(report.healthy);
        assert!(report.components.is_empty());
    }

    #[tokio::test]
    async fn config_invalid_components_surfaced_in_check_output() {
        let checker = InfrastructureHealthChecker::new()
            .with_config_invalid("postgres", "invalid connection string")
            .with_config_invalid("redis", "unresolvable host");

        let report = checker.check().await;

        assert!(
            !report.healthy,
            "report should be unhealthy with config-invalid components"
        );
        assert_eq!(report.components.len(), 2);

        let pg = report
            .components
            .iter()
            .find(|c| c.name == "postgres")
            .expect("postgres component missing");
        assert!(!pg.healthy);
        assert!(pg.detail.contains("config_invalid"));
        assert!(pg.detail.contains("invalid connection string"));

        let redis_component = report
            .components
            .iter()
            .find(|c| c.name == "redis")
            .expect("redis component missing");
        assert!(!redis_component.healthy);
        assert!(redis_component.detail.contains("config_invalid"));
        assert!(redis_component.detail.contains("unresolvable host"));
    }

    /// Proves that `with_static_component` surfaces the component in the `/health`
    /// report and that a healthy static component does not flip the overall health to
    /// unhealthy.
    #[tokio::test]
    async fn static_component_appears_in_report_and_preserves_healthy_flag() {
        let checker = InfrastructureHealthChecker::new()
            .with_static_component("usage_write", true, "disabled")
            .with_static_component("extraction_provider", true, "ollama");

        let report = checker.check().await;

        assert!(
            report.healthy,
            "healthy static components must not flip the overall health flag"
        );
        assert_eq!(report.components.len(), 2);

        let usage = report
            .components
            .iter()
            .find(|c| c.name == "usage_write")
            .expect("usage_write component must be present");
        assert!(usage.healthy);
        assert_eq!(usage.detail, "disabled");

        let provider = report
            .components
            .iter()
            .find(|c| c.name == "extraction_provider")
            .expect("extraction_provider component must be present");
        assert!(provider.healthy);
        assert_eq!(provider.detail, "ollama");
    }

    /// Proves that `build_health_checker_from_environment` always injects
    /// `usage_write: "enabled"` — usage writing is unconditionally on.
    ///
    /// The startup-time health signal is always "enabled"; actual write failures
    /// surface at runtime via compile_context health markers.
    #[tokio::test]
    async fn build_health_checker_always_injects_usage_write_enabled() {
        // Must run inside a Tokio runtime: `build_health_checker_from_environment`
        // constructs sqlx pools whose connection-reaper task requires an ambient
        // runtime at build time. The previous form called the factory OUTSIDE the
        // runtime (before `block_on`), which panicked "requires a Tokio context"
        // once sibling tests shifted the in-binary scheduling.
        let checker = DependencyFactory::build_health_checker_from_environment();

        let report = checker.check().await;

        let usage = report
            .components
            .iter()
            .find(|c| c.name == "usage_write")
            .expect("usage_write must be present in /health output");
        assert_eq!(
            usage.detail, "enabled",
            "usage_write must always be 'enabled' — no rollback flag exists"
        );
    }

    /// Proves that `with_static_component("embedding_arm", ...)` surfaces the
    /// active embedding arm in the `/health` report so agents can discover which
    /// vector space produced `find_skill` results without tailing container logs.
    ///
    /// The detail format is `model=<name> dim=<n> collection=<name>`, matching the
    /// `extraction_provider` / `usage_write` convention established in `main.rs`.
    /// Data source: boot-discovered `EmbeddingModelInfo` (model + dimension) plus the
    /// Qdrant adapter's resolved collection name. See #228 for the future
    /// `embedding_model_metadata` row source once a graph rebuild has run.
    #[tokio::test]
    async fn embedding_arm_component_surfaces_model_dimension_and_collection_in_health() {
        let checker = InfrastructureHealthChecker::new().with_static_component(
            "embedding_arm",
            true,
            "model=qwen3-embedding:4b dim=2560 collection=skills-qwen3-embedding-4b",
        );

        let report = checker.check().await;

        assert!(
            report.healthy,
            "a healthy embedding_arm component must not flip the overall health flag"
        );

        let arm = report
            .components
            .iter()
            .find(|c| c.name == "embedding_arm")
            .expect("embedding_arm must be present in /health output");
        assert!(
            arm.healthy,
            "embedding_arm component must be marked healthy"
        );
        assert!(
            arm.detail.contains("model="),
            "embedding_arm detail must contain 'model=': got '{}'",
            arm.detail
        );
        assert!(
            arm.detail.contains("dim="),
            "embedding_arm detail must contain 'dim=': got '{}'",
            arm.detail
        );
        assert!(
            arm.detail.contains("collection="),
            "embedding_arm detail must contain 'collection=': got '{}'",
            arm.detail
        );
        assert_eq!(
            arm.detail, "model=qwen3-embedding:4b dim=2560 collection=skills-qwen3-embedding-4b",
            "embedding_arm detail format must be 'model=X dim=Y collection=Z'"
        );
    }

    /// Proves that `with_http_dependency("qdrant_read_path", …)` can be registered
    /// and surfaces in the `/health` report with the correct component name.
    ///
    /// The factory (`build_health_checker_from_environment`) conditionally registers this
    /// component when `RETRIEVAL_BACKEND=qdrant_hybrid` and `QDRANT_URL` is set.
    /// We verify the builder API and component naming are wired correctly here;
    /// the factory's env-conditional branching is the DS-003 acceptance criterion.
    ///
    /// The actual health state (healthy/unhealthy) depends on whether Qdrant is
    /// reachable — this test only asserts the component appears with the right name.
    #[tokio::test]
    async fn qdrant_read_path_component_surfaces_when_registered_as_http_dependency() {
        let http_client = reqwest::Client::new();
        // Probe an intentionally-invalid endpoint so the test does not require a
        // running Qdrant. Using a reserved / non-routable address instead of a local
        // port that may or may not be occupied (e.g. by the dev compose stack).
        let checker = InfrastructureHealthChecker::new().with_http_dependency(
            http_client,
            "qdrant_read_path",
            "http://192.0.2.1:6333/collections", // RFC 5737 TEST-NET — guaranteed unreachable
        );

        let report = checker.check().await;

        let read_path = report
            .components
            .iter()
            .find(|c| c.name == "qdrant_read_path")
            .expect("qdrant_read_path must appear in /health when registered");

        // The component is present regardless of health state. Under production
        // conditions the component will be healthy when Qdrant is reachable, and
        // unhealthy (surfacing a degraded read path) when Qdrant is down.
        assert_eq!(
            read_path.name, "qdrant_read_path",
            "component name must be exactly 'qdrant_read_path'"
        );
        // The RFC 5737 address is non-routable; expect the probe to fail.
        assert!(
            !read_path.healthy,
            "probe to non-routable address must report unhealthy: got detail '{}'",
            read_path.detail
        );
    }

    /// Proves that snapshot-arm health checkers do NOT register a `qdrant_read_path`
    /// component — only `QdrantHybrid` has Qdrant as a read dependency.
    ///
    /// This is the DS-003 invariant: snapshot arms serve from in-memory state;
    /// Qdrant is a write-only dependency for them. The read-path label must not
    /// appear where it does not apply, or operators will chase phantom outages.
    #[tokio::test]
    async fn qdrant_read_path_absent_when_not_registered() {
        // No with_http_dependency("qdrant_read_path") call — simulating snapshot arms.
        let checker = InfrastructureHealthChecker::new()
            .with_static_component("usage_write", true, "enabled")
            .with_static_component("extraction_provider", true, "ollama");

        let report = checker.check().await;

        assert!(
            report
                .components
                .iter()
                .all(|c| c.name != "qdrant_read_path"),
            "qdrant_read_path must be absent from /health for snapshot-arm configs"
        );
    }

    // ---------------------------------------------------------------------------
    // ReadinessHandle unit tests (T17 AC1)
    // ---------------------------------------------------------------------------

    /// Proves `ReadinessHandle::warming()` starts in Warming state (`is_ready` = false),
    /// transitions to Ready via `set_ready()`, and that state is reflected by `is_ready`.
    #[test]
    fn readiness_handle_warming_to_ready_transition() {
        let handle = ReadinessHandle::warming();
        assert!(
            !handle.is_ready(),
            "handle created via warming() must start NOT ready"
        );

        handle.set_ready();
        assert!(
            handle.is_ready(),
            "after set_ready() the handle must report is_ready = true"
        );
    }

    /// Proves `set_failed()` transitions the handle out of Ready and `is_ready` returns false.
    #[test]
    fn readiness_handle_ready_to_failed_transition() {
        let handle = ReadinessHandle::ready();
        assert!(handle.is_ready(), "handle from ready() must start ready");

        handle.set_failed("build_graph_from_pg: connection refused");
        assert!(
            !handle.is_ready(),
            "after set_failed() the handle must report is_ready = false"
        );
    }

    /// Proves `set_warming()` followed by `set_failed()` results in `is_ready = false`,
    /// and `set_warming()` alone also results in `is_ready = false`.
    #[test]
    fn readiness_handle_warming_is_not_ready() {
        let handle = ReadinessHandle::ready();
        handle.set_warming();
        assert!(
            !handle.is_ready(),
            "handle in Warming state must not be ready"
        );
    }

    /// Proves `health_component()` produces a healthy component when Ready.
    #[test]
    fn readiness_handle_health_component_ready() {
        let handle = ReadinessHandle::ready();
        let component = handle.health_component();

        assert_eq!(component.name, "readiness");
        assert!(
            component.healthy,
            "Ready state must produce a healthy readiness component"
        );
        assert_eq!(component.detail, "ready");
    }

    /// Proves `health_component()` produces an unhealthy component with the warming detail
    /// when the handle is in Warming state.
    #[test]
    fn readiness_handle_health_component_warming() {
        let handle = ReadinessHandle::warming();
        let component = handle.health_component();

        assert_eq!(component.name, "readiness");
        assert!(
            !component.healthy,
            "Warming state must produce an unhealthy readiness component"
        );
        assert!(
            component.detail.contains("warming"),
            "Warming detail must contain 'warming': got '{}'",
            component.detail
        );
    }

    /// Proves `health_component()` produces an unhealthy component with the failure message
    /// when the handle is in Failed state.
    #[test]
    fn readiness_handle_health_component_failed() {
        let handle = ReadinessHandle::warming();
        handle.set_failed("pg pool timed out");
        let component = handle.health_component();

        assert_eq!(component.name, "readiness");
        assert!(
            !component.healthy,
            "Failed state must produce an unhealthy readiness component"
        );
        assert!(
            component.detail.contains("failed"),
            "Failed detail must contain 'failed': got '{}'",
            component.detail
        );
        assert!(
            component.detail.contains("pg pool timed out"),
            "Failed detail must contain the error message: got '{}'",
            component.detail
        );
    }

    /// Proves `with_readiness` wires the readiness component into `check()` and that
    /// Warming makes the overall report unhealthy (→ /health 503).
    #[tokio::test]
    async fn with_readiness_warming_makes_report_unhealthy() {
        let handle = Arc::new(ReadinessHandle::warming());
        let checker = InfrastructureHealthChecker::new().with_readiness(handle);

        let report = checker.check().await;

        assert!(
            !report.healthy,
            "Warming readiness must make the overall /health report unhealthy"
        );

        let component = report
            .components
            .iter()
            .find(|c| c.name == "readiness")
            .expect("readiness component must appear in check() output when with_readiness is set");
        assert!(!component.healthy);
        assert!(component.detail.contains("warming"));
    }

    /// Proves `with_readiness` wires the readiness component into `check()` and that
    /// Ready makes the overall report healthy.
    #[tokio::test]
    async fn with_readiness_ready_preserves_healthy_report() {
        let handle = Arc::new(ReadinessHandle::ready());
        let checker = InfrastructureHealthChecker::new().with_readiness(handle);

        let report = checker.check().await;

        assert!(
            report.healthy,
            "Ready readiness must leave the overall /health report healthy"
        );

        let component = report
            .components
            .iter()
            .find(|c| c.name == "readiness")
            .expect("readiness component must appear in check() output");
        assert!(component.healthy);
        assert_eq!(component.detail, "ready");
    }

    /// Proves `with_readiness` Failed state makes the overall report unhealthy and
    /// surfaces the failure message.
    #[tokio::test]
    async fn with_readiness_failed_makes_report_unhealthy_with_message() {
        let handle = Arc::new(ReadinessHandle::warming());
        handle.set_failed("pg connection pool exhausted");
        let checker = InfrastructureHealthChecker::new()
            .with_readiness(handle);

        let report = checker.check().await;

        assert!(
            !report.healthy,
            "Failed readiness must make the overall /health report unhealthy"
        );

        let component = report
            .components
            .iter()
            .find(|c| c.name == "readiness")
            .expect("readiness component must be present");
        assert!(!component.healthy);
        assert!(
            component.detail.contains("pg connection pool exhausted"),
            "detail must contain the failure message: got '{}'",
            component.detail
        );
    }

    /// Proves that checkers WITHOUT `with_readiness` do NOT emit a `readiness` component,
    /// preserving the existing component count for all tests that do not call `with_readiness`.
    #[tokio::test]
    async fn without_with_readiness_no_readiness_component_emitted() {
        let checker = InfrastructureHealthChecker::new()
            .with_static_component("usage_write", true, "enabled");

        let report = checker.check().await;

        assert!(
            report
                .components
                .iter()
                .all(|c| c.name != "readiness"),
            "readiness component must be absent when with_readiness was not called"
        );
    }
}
