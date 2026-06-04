use std::time::Duration;

use sqlx::{PgPool, postgres::PgPoolOptions};
use thiserror::Error;

const MIGRATION_001: &str = include_str!("../../migrations/001_initial_schema.sql");
const MIGRATION_002: &str = include_str!("../../migrations/002_transcript_ingest_queue.sql");
const MIGRATION_003: &str = include_str!("../../migrations/003_usage_fields.sql");
const MIGRATION_004: &str = include_str!("../../migrations/004_session_logs_status_check.sql");
/// Migration 005: adds `skills.source_paths TEXT[] NOT NULL DEFAULT '{}'`.
/// Per-skill SKILL.md provenance so the retrieval boot adapter uses true paths
/// instead of the scope-root stand-in. Pre-migration rows get an empty array
/// and fall back to the scope-root behavior in `build_graph_from_pg`.
const MIGRATION_005: &str = include_str!("../../migrations/005_skill_source_paths.sql");

/// Ordered migration set applied on every boot. Each entry is idempotent
/// (`IF NOT EXISTS` / `ADD COLUMN IF NOT EXISTS`) so re-running is safe; ordering
/// matters because later migrations depend on objects created by earlier ones
/// (e.g. 002 reuses the `set_updated_at_timestamp()` function from 001, and
/// 003 adds typed columns to tables declared by 001, 004 adds a CHECK constraint
/// to session_logs, 005 adds the source_paths provenance column to skills).
const MIGRATIONS: &[&str] = &[
    MIGRATION_001,
    MIGRATION_002,
    MIGRATION_003,
    MIGRATION_004,
    MIGRATION_005,
];

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
                // Test connections before checkout. This evicts connections that are in
                // an "idle in transaction (aborted)" state — e.g. left dirty after a
                // failed multi-statement migration run. Without this guard, the pool
                // can recycle a dirty connection and every subsequent query fails with
                // "current transaction is aborted".
                .test_before_acquire(true)
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
        for migration in MIGRATIONS {
            sqlx::raw_sql(migration)
                .execute(&self.pool)
                .await
                .map_err(|error| PostgresError::Migration(error.to_string()))?;
        }
        Ok(())
    }

    /// Truncates all application tables. Intended for test teardown only.
    ///
    /// Includes `session_logs` and `skill_usage` so E2E tests run with a clean
    /// usage slate and do not leak usage rows across runs (T06).
    #[cfg(any(test, feature = "test-utils"))]
    pub async fn truncate_all_tables(&self) -> Result<(), PostgresError> {
        sqlx::query(
            "TRUNCATE TABLE community_skills, skill_subunits, communities, subunits, skills, \
             outbox_events, rebuild_locks, transcript_ingest_queue, \
             session_logs, skill_usage CASCADE",
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

    #[test]
    fn migration_002_declares_transcript_ingest_queue() {
        assert!(
            MIGRATION_002.contains("transcript_ingest_queue"),
            "migration 002 should declare the transcript ingest queue table"
        );
        assert!(
            MIGRATION_002.contains("content_hash TEXT NOT NULL UNIQUE"),
            "dedup is keyed on a UNIQUE content_hash"
        );
    }

    #[test]
    fn migration_set_is_ordered_001_through_005() {
        assert_eq!(
            MIGRATIONS,
            &[
                MIGRATION_001,
                MIGRATION_002,
                MIGRATION_003,
                MIGRATION_004,
                MIGRATION_005
            ]
        );
    }

    #[test]
    fn migration_003_adds_typed_usage_columns() {
        assert!(
            MIGRATION_003.contains("prompt_hash"),
            "migration 003 should add session_logs.prompt_hash"
        );
        assert!(
            MIGRATION_003.contains("latency_ms"),
            "migration 003 should add session_logs.latency_ms"
        );
        assert!(
            MIGRATION_003.contains("relevance_score"),
            "migration 003 should add skill_usage.relevance_score"
        );
        assert!(
            MIGRATION_003.contains("ADD COLUMN IF NOT EXISTS"),
            "migration 003 must use ADD COLUMN IF NOT EXISTS (non-rewriting)"
        );
    }

    #[test]
    fn truncate_all_tables_sql_includes_usage_tables() {
        // Compile-time check: the truncate statement must list both usage tables
        // so E2E teardown does not leak usage rows across runs.
        let sql = "TRUNCATE TABLE community_skills, skill_subunits, communities, subunits, skills, \
             outbox_events, rebuild_locks, transcript_ingest_queue, \
             session_logs, skill_usage CASCADE";
        assert!(sql.contains("session_logs"));
        assert!(sql.contains("skill_usage"));
    }
}
