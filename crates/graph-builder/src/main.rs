use std::{path::PathBuf, sync::Arc, time::Duration};

use domain::{ScopeRoot, ScopeType};
use graph_builder::{
    GraphRebuildOrchestrator, PostgresDurableGraphState, SkillFileChange, SkillWatcher,
    WatcherRecovery,
};
use infrastructure::{
    CircuitState, DependencyFactory, EventEnvelope, HealthReport, InfrastructureHealthChecker,
    OllamaEmbeddingConfig, OllamaEmbeddingService, PostgresAdapter, PostgresConfig,
    PostgresGraphWriteCoordinator, PostgresRebuildCoordinator, QdrantAdapter, QdrantConfig,
    RebuildCoordinator, RedisStreamError, RedisStreamsAdapter, RedisStreamsConfig,
    logging::init_logging,
};
use serde::Serialize;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::RwLock,
    time::sleep,
};

fn build_scope_roots() -> Vec<ScopeRoot> {
    let repo_root = PathBuf::from(std::env::var("GRAPH_BUILDER_PROJECT_ROOT").unwrap_or_else(
        |_| {
            std::env::current_dir()
                .unwrap_or_default()
                .display()
                .to_string()
        },
    ));
    vec![
        ScopeRoot::new("project", ScopeType::Project, repo_root),
        ScopeRoot::new(
            "global",
            ScopeType::Global,
            std::env::var("GRAPH_BUILDER_GLOBAL_ROOT")
                .map(PathBuf::from)
                .unwrap_or_else(|_| std::env::current_dir().unwrap_or_default().join("docs")),
        ),
    ]
}

fn polling_interval() -> Duration {
    std::env::var("GRAPH_BUILDER_POLL_INTERVAL_MS")
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or_else(|| Duration::from_secs(15))
}

/// Builds the Redis Streams adapter graph-builder publishes `graph.rebuilt` to.
///
/// This is the R-2 fix: before T02, `rebuild_from_changes` pushed each
/// `graph.rebuilt` envelope into an in-memory `Vec` the rebuild loop never
/// drained, so the online server never learned about rebuilds. The adapter must
/// use the SAME stream/group the online subscriber reads (`skill-layer-events` /
/// `skill-layer`) so the published event actually reaches it.
fn build_redis_streams_adapter() -> Result<RedisStreamsAdapter, RedisStreamError> {
    let redis_config = RedisStreamsConfig {
        redis_url: std::env::var("REDIS_URL")
            .unwrap_or_else(|_| RedisStreamsConfig::default().redis_url),
        ..RedisStreamsConfig::default()
    };
    RedisStreamsAdapter::new(redis_config)
}

/// Drains the in-memory published-events buffer to Redis via `XADD`.
///
/// Publishes envelopes in order. On the first failure, the successfully
/// published prefix is drained in one shot (`drain(..published_count)`) and
/// the failed envelope is left at the front so the next cycle retries it.
/// Failures are logged but never panic — the rebuild loop keeps running.
///
/// Returns the highest `graph_version` extracted from any successfully
/// published `graph.rebuilt` envelope, or `None` if none were published.
async fn drain_published_events(
    redis_streams: &RedisStreamsAdapter,
    published_events: &mut Vec<EventEnvelope>,
) -> Option<i64> {
    let mut published_count = 0;
    let mut max_published_graph_version: Option<i64> = None;
    for envelope in published_events.iter() {
        match redis_streams.publish(envelope).await {
            Ok(stream_id) => {
                tracing::info!(
                    event_type = %envelope.event_type,
                    idempotency_key = %envelope.idempotency_key,
                    %stream_id,
                    "published graph event to redis stream"
                );
                if envelope.event_type == "graph.rebuilt"
                    && let Some(version) = envelope
                        .payload
                        .get("graph_version")
                        .and_then(|v| v.as_i64())
                {
                    max_published_graph_version =
                        Some(max_published_graph_version.unwrap_or(0).max(version));
                }
                published_count += 1;
            }
            Err(error) => {
                tracing::error!(
                    event_type = %envelope.event_type,
                    %error,
                    "failed to publish graph event to redis; will retry next cycle"
                );
                break;
            }
        }
    }
    // Remove the successfully published prefix in one allocation-free drain.
    published_events.drain(..published_count);
    max_published_graph_version
}

#[derive(Debug, Clone, Default)]
struct GraphBuilderHealthState {
    last_rebuild_error: Option<String>,
    circuit_state: Option<CircuitState>,
}

#[derive(Debug, Serialize)]
struct GraphBuilderHealthResponse {
    healthy: bool,
    detail: String,
    circuit_state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_rebuild_error: Option<String>,
    dependencies: HealthReport,
}

/// Replays a `graph.rebuilt` event to Redis if PG `graph_version` is ahead of
/// the last version we published.
///
/// Addresses bug #156 replay-safety: if a previous cycle advanced PG
/// `graph_state.graph_version` but failed before publishing `graph.rebuilt`
/// (e.g. due to the outbox idempotency conflict), the mcp-server's snapshot
/// freezes indefinitely. On the next cycle start, this function detects the
/// gap and re-publishes the current version.
///
/// The mcp-server `graph_refresh_subscriber` and `swap_graph` are idempotent
/// for same-or-older versions, so replaying is always safe.
async fn maybe_replay_graph_rebuilt(
    rebuild_coordinator: &PostgresRebuildCoordinator,
    redis_streams: &RedisStreamsAdapter,
    last_published_version: &mut i64,
) {
    let pg_version = match rebuild_coordinator.current_graph_version().await {
        Ok(version) => version,
        Err(error) => {
            tracing::warn!(%error, "could not read PG graph_version for replay check; skipping");
            return;
        }
    };

    if pg_version <= *last_published_version {
        return;
    }

    tracing::warn!(
        pg_version,
        last_published_version = *last_published_version,
        "PG graph_version is ahead of last published version; replaying graph.rebuilt"
    );

    let envelope = EventEnvelope::new(
        "graph.rebuilt",
        format!("graph.rebuilt:{pg_version}"),
        serde_json::json!({
            "graph_version": pg_version,
            "replayed": true,
        }),
    );
    match redis_streams.publish(&envelope).await {
        Ok(stream_id) => {
            tracing::info!(
                pg_version,
                %stream_id,
                "replayed graph.rebuilt to redis for frozen snapshot recovery"
            );
            *last_published_version = pg_version;
        }
        Err(error) => {
            tracing::error!(
                pg_version,
                %error,
                "failed to replay graph.rebuilt; will retry next cycle"
            );
        }
    }
}

/// Builds a real Ollama embedding service from the `OLLAMA_URL` environment variable.
///
/// Fails loud when `OLLAMA_URL` is unset — there is no fallback embedder in production.
fn build_embedding_service() -> Result<OllamaEmbeddingService, Box<dyn std::error::Error>> {
    let base_url = std::env::var("OLLAMA_URL")
        .map_err(|_| "OLLAMA_URL must be set to connect to the embedding service")?;
    let config = OllamaEmbeddingConfig {
        base_url,
        model: "nomic-embed-text".to_owned(),
        timeout_ms: 5_000,
        batch_timeout_ms: 10_000,
        max_concurrency: 4,
    };
    OllamaEmbeddingService::from_config(config).map_err(|e| e.to_string().into())
}

async fn run_rebuild_cycle(
    watcher: &mut SkillWatcher,
    recovery: &mut WatcherRecovery,
    orchestrator: &mut GraphRebuildOrchestrator<'_, PostgresDurableGraphState<'_, QdrantAdapter>>,
) -> Result<Option<i64>, String> {
    let first_scan = watcher
        .collect_file_changes()
        .map_err(|error| error.to_string())?;
    let recovered = recovery.reconcile(
        &watcher.previous_snapshot(),
        &watcher.current_snapshot(),
        &watcher.scopes(),
    );
    let mut all_changes: Vec<SkillFileChange> = first_scan;
    all_changes.extend(recovered);

    if all_changes.is_empty() {
        return Ok(None);
    }

    orchestrator
        .rebuild_from_changes(&watcher.scopes(), &all_changes)
        .await
        .map_err(|error| error.to_string())
        .map(|outcome| {
            tracing::info!(
                graph_version = outcome.graph_version,
                skills_count = outcome.skills_count,
                communities_count = outcome.communities_count,
                "graph rebuilt"
            );
            Some(outcome.graph_version)
        })
}

async fn serve_health_endpoint(
    health_checker: InfrastructureHealthChecker,
    runtime_health_state: Arc<RwLock<GraphBuilderHealthState>>,
) -> std::io::Result<()> {
    let address = std::env::var("GRAPH_BUILDER_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:8080".to_owned())
        .parse::<std::net::SocketAddr>()
        .map_err(std::io::Error::other)?;
    let listener = TcpListener::bind(address).await?;

    loop {
        let (mut socket, _) = listener.accept().await?;
        let mut request = vec![0_u8; 2048];
        let _ = socket.read(&mut request).await?;
        let request_text = String::from_utf8_lossy(&request);
        let request_line = request_text.lines().next().unwrap_or_default();

        if !request_line.starts_with("GET /health") {
            socket.write_all(
                b"HTTP/1.1 404 Not Found\r\ncontent-length: 9\r\nconnection: close\r\n\r\nnot found",
            )
            .await?;
            continue;
        }

        let dependencies = health_checker.check().await;
        let state = runtime_health_state.read().await.clone();
        let circuit_state = state
            .circuit_state
            .map(|value| format!("{value:?}"))
            .unwrap_or_else(|| "Closed".to_owned());
        let healthy = dependencies.healthy
            && state.last_rebuild_error.is_none()
            && state.circuit_state != Some(CircuitState::Open);
        let detail = if healthy {
            "ok".to_owned()
        } else {
            "degraded (graph_builder_runtime)".to_owned()
        };
        let payload = serde_json::to_string(&GraphBuilderHealthResponse {
            healthy,
            detail,
            circuit_state,
            last_rebuild_error: state.last_rebuild_error,
            dependencies,
        })
        .map_err(std::io::Error::other)?;
        let status_line = if healthy {
            "HTTP/1.1 200 OK"
        } else {
            "HTTP/1.1 503 Service Unavailable"
        };
        let response = format!(
            "{status_line}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            payload.len(),
            payload
        );
        socket.write_all(response.as_bytes()).await?;
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_logging("graph-builder", "info")?;

    let scopes = build_scope_roots();
    let mut watcher = SkillWatcher::new(scopes)?;
    let mut recovery = WatcherRecovery::default();

    let db_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        "postgres://skill_layer:skill_layer@localhost:15432/skill_layer".to_owned()
    });
    let qdrant_url =
        std::env::var("QDRANT_URL").unwrap_or_else(|_| "http://localhost:16333".to_owned());

    let pg_adapter = PostgresAdapter::connect(&PostgresConfig {
        database_url: db_url,
        ..PostgresConfig::default()
    })
    .await?;

    let rebuild_coordinator = PostgresRebuildCoordinator::new(pg_adapter.pool().clone());
    let outbox_coordinator = PostgresGraphWriteCoordinator::new(pg_adapter.pool().clone());
    let qdrant_adapter = QdrantAdapter::new(
        reqwest::Client::new(),
        QdrantConfig {
            endpoint: qdrant_url,
            ..QdrantConfig::default()
        },
    )
    .map_err(|error| error.to_string())?;

    let embedding_service = build_embedding_service()?;

    let _ = pg_adapter.run_migrations().await;
    qdrant_adapter
        .ensure_collection(&qdrant_adapter.config.collection_name, 768)
        .await
        .map_err(|error| format!("qdrant collection setup: {error}"))?;

    let mut durable_state =
        PostgresDurableGraphState::new(&rebuild_coordinator, &outbox_coordinator, &qdrant_adapter);
    let mut published_events: Vec<EventEnvelope> = Vec::new();
    // Tracks the highest `graph_version` for which we have successfully published
    // a `graph.rebuilt` event to Redis. Used by `maybe_replay_graph_rebuilt` to
    // detect and recover from cycles that advanced PG version but failed before
    // publishing (bug #156 replay-safety).
    let mut last_published_graph_version: i64 = 0;

    let redis_streams = build_redis_streams_adapter()?;
    redis_streams.ensure_consumer_group().await?;

    let runtime_health_state = Arc::new(RwLock::new(GraphBuilderHealthState::default()));
    let health_server_state = Arc::clone(&runtime_health_state);
    tokio::spawn(async move {
        if let Err(error) = serve_health_endpoint(
            DependencyFactory::build_health_checker_from_environment(),
            health_server_state,
        )
        .await
        {
            tracing::error!(%error, "graph-builder health endpoint failed");
        }
    });

    loop {
        // Replay-safety (bug #156): if PG graph_version is ahead of the last version
        // we published to Redis, re-publish graph.rebuilt so the mcp-server snapshot
        // can unfreeze even after a previous cycle that advanced PG but failed before
        // publishing. Safe to call every cycle — it exits immediately when versions match.
        maybe_replay_graph_rebuilt(
            &rebuild_coordinator,
            &redis_streams,
            &mut last_published_graph_version,
        )
        .await;

        // Scope the orchestrator so its borrow of `published_events` ends before
        // the drain below. A fresh orchestrator per cycle is cheap (it only
        // borrows the durable state and the buffer) and lets the loop own the
        // buffer it must publish from.
        let cycle_result = {
            let mut orchestrator = GraphRebuildOrchestrator::new(
                &mut durable_state,
                &mut published_events,
                &embedding_service,
            );
            run_rebuild_cycle(&mut watcher, &mut recovery, &mut orchestrator).await
        };
        match cycle_result {
            Ok(Some(_version)) => {
                tracing::info!("rebuild cycle completed successfully");
            }
            Ok(None) => {}
            Err(error) => {
                tracing::error!(error = %error, "rebuild cycle failed");
            }
        }
        // Publish the freshly-pushed `graph.rebuilt` envelope(s) to Redis so the
        // online server's subscriber can refresh without a restart (R-2 fix).
        if let Some(version) = drain_published_events(&redis_streams, &mut published_events).await {
            last_published_graph_version = last_published_graph_version.max(version);
        }

        sleep(polling_interval()).await;
    }
}
