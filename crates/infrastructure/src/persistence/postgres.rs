use std::time::Duration;

use sqlx::{PgPool, postgres::PgPoolOptions};
use thiserror::Error;

const MIGRATION_001: &str = include_str!("../../migrations/001_initial_schema.sql");

#[derive(Debug, Clone)]
pub struct PostgresConfig {
    pub database_url: String,
    pub max_connections: u32,
    pub min_connections: u32,
    pub connect_timeout_secs: u64,
    pub acquire_timeout_secs: u64,
}

impl Default for PostgresConfig {
    fn default() -> Self {
        Self {
            database_url: String::new(),
            max_connections: 20,
            min_connections: 1,
            connect_timeout_secs: 5,
            acquire_timeout_secs: 3,
        }
    }
}

#[derive(Debug, Error)]
pub enum PostgresError {
    #[error("invalid postgres configuration: {0}")]
    InvalidConfiguration(String),
    #[error("postgres connection failure: {0}")]
    Connection(#[from] sqlx::Error),
    #[error("postgres migration failure: {0}")]
    Migration(String),
}

#[derive(Debug, Clone)]
pub struct PostgresAdapter {
    pool: PgPool,
}

impl PostgresAdapter {
    pub async fn connect(config: &PostgresConfig) -> Result<Self, PostgresError> {
        if config.database_url.trim().is_empty() {
            return Err(PostgresError::InvalidConfiguration(
                "database_url must not be blank".to_owned(),
            ));
        }

        if config.max_connections == 0
            || config.acquire_timeout_secs == 0
            || config.connect_timeout_secs == 0
        {
            return Err(PostgresError::InvalidConfiguration(
                "pool and timeout values must be greater than zero".to_owned(),
            ));
        }

        let pool = tokio::time::timeout(
            Duration::from_secs(config.connect_timeout_secs),
            PgPoolOptions::new()
                .max_connections(config.max_connections)
                .min_connections(config.min_connections)
                .acquire_timeout(Duration::from_secs(config.acquire_timeout_secs))
                .connect(&config.database_url),
        )
        .await
        .map_err(|_| PostgresError::Connection(sqlx::Error::PoolTimedOut))??;

        Ok(Self { pool })
    }

    pub fn from_pool(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub async fn ping(&self) -> Result<(), PostgresError> {
        sqlx::query("SELECT 1").execute(&self.pool).await?;
        Ok(())
    }

    pub async fn run_migrations(&self) -> Result<(), PostgresError> {
        sqlx::raw_sql(MIGRATION_001)
            .execute(&self.pool)
            .await
            .map_err(|error| PostgresError::Migration(error.to_string()))?;
        Ok(())
    }

    /// Truncates all application tables. Intended for test teardown only.
    #[cfg(any(test, feature = "test-utils"))]
    pub async fn truncate_all_tables(&self) -> Result<(), PostgresError> {
        sqlx::query(
            "TRUNCATE TABLE community_skills, skill_subunits, communities, subunits, skills, outbox_events, rebuild_locks CASCADE"
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_contains_required_contract_tables() {
        for table in [
            "skills",
            "subunits",
            "communities",
            "skill_subunits",
            "community_skills",
            "session_logs",
            "skill_usage",
            "audit_log",
            "outbox_events",
            "rebuild_locks",
        ] {
            assert!(
                MIGRATION_001.contains(table),
                "migration should declare {table}"
            );
        }
    }
}
