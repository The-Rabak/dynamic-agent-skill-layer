use chrono::{DateTime, Utc};
use redis::AsyncCommands;
use serde::Serialize;
use sqlx::PgPool;

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
    ollama_url: Option<String>,
    http_client: Option<reqwest::Client>,
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
        self.ollama_url = Some(base_url.into());
        self
    }

    pub async fn check(&self) -> HealthReport {
        let mut components = Vec::new();

        if let Some(pool) = &self.postgres {
            let postgres_health = match sqlx::query("SELECT 1").execute(pool).await {
                Ok(_) => HealthComponent {
                    name: "postgres".to_owned(),
                    healthy: true,
                    detail: "reachable".to_owned(),
                },
                Err(error) => HealthComponent {
                    name: "postgres".to_owned(),
                    healthy: false,
                    detail: error.to_string(),
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
                        Err(error) => HealthComponent {
                            name: "redis".to_owned(),
                            healthy: false,
                            detail: error.to_string(),
                        },
                    }
                }
                Err(error) => HealthComponent {
                    name: "redis".to_owned(),
                    healthy: false,
                    detail: error.to_string(),
                },
            };
            components.push(redis_health);
        }

        if let (Some(client), Some(url)) = (&self.http_client, &self.ollama_url) {
            let endpoint = format!("{}/api/tags", url.trim_end_matches('/'));
            let ollama_health = match client.get(endpoint).send().await {
                Ok(response) if response.status().is_success() => HealthComponent {
                    name: "ollama".to_owned(),
                    healthy: true,
                    detail: "reachable".to_owned(),
                },
                Ok(response) => HealthComponent {
                    name: "ollama".to_owned(),
                    healthy: false,
                    detail: format!("status {}", response.status()),
                },
                Err(error) => HealthComponent {
                    name: "ollama".to_owned(),
                    healthy: false,
                    detail: error.to_string(),
                },
            };
            components.push(ollama_health);
        }

        let healthy = components.iter().all(|component| component.healthy);

        HealthReport {
            healthy,
            checked_at: Utc::now(),
            components,
        }
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
}
