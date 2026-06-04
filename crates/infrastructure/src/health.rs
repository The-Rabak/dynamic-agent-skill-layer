use std::sync::Arc;

use chrono::{DateTime, Utc};
use domain::EmbeddingService;
use redis::AsyncCommands;
use serde::Serialize;
use sqlx::PgPool;

use crate::embeddings::ollama::{OllamaEmbeddingConfig, OllamaEmbeddingService};

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
            // Label as "qdrant_write_side": Qdrant is the durable write-side vector
            // store (outbox drain target). It is NOT queried at read time under
            // Option A (ADR-0001). The label must not imply a read-path dependency.
            checker = checker.with_http_dependency(
                http_client.clone(),
                "qdrant_write_side",
                format!("{}/collections", qdrant_url.trim_end_matches('/')),
            );
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
}
