use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RebuildLock {
    pub lock_name: String,
    pub owner_id: Uuid,
    pub acquired_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Error)]
pub enum RebuildError {
    #[error("invalid rebuild contract: {0}")]
    InvalidContract(String),
    #[error("rebuild persistence error: {0}")]
    Persistence(#[from] sqlx::Error),
}

#[async_trait]
pub trait RebuildCoordinator: Send + Sync {
    async fn try_acquire_lock(
        &self,
        lock_name: &str,
        owner_id: Uuid,
        lease_duration: Duration,
    ) -> Result<bool, RebuildError>;
    async fn renew_lock(
        &self,
        lock_name: &str,
        owner_id: Uuid,
        lease_duration: Duration,
    ) -> Result<bool, RebuildError>;
    async fn release_lock(&self, lock_name: &str, owner_id: Uuid) -> Result<(), RebuildError>;
    async fn current_graph_version(&self) -> Result<i64, RebuildError>;
    async fn bump_graph_version(&self) -> Result<i64, RebuildError>;
}

#[derive(Debug, Clone)]
pub struct PostgresRebuildCoordinator {
    pool: PgPool,
}

impl PostgresRebuildCoordinator {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl RebuildCoordinator for PostgresRebuildCoordinator {
    async fn try_acquire_lock(
        &self,
        lock_name: &str,
        owner_id: Uuid,
        lease_duration: Duration,
    ) -> Result<bool, RebuildError> {
        if lock_name.trim().is_empty() {
            return Err(RebuildError::InvalidContract(
                "lock_name must not be blank".to_owned(),
            ));
        }

        let lease_end = Utc::now()
            + chrono::Duration::from_std(lease_duration)
                .map_err(|err| RebuildError::InvalidContract(err.to_string()))?;

        let result = sqlx::query(
            r#"
            INSERT INTO rebuild_locks (lock_name, owner_id, acquired_at, expires_at)
            VALUES ($1, $2, NOW(), $3)
            ON CONFLICT (lock_name) DO UPDATE
            SET owner_id = EXCLUDED.owner_id,
                acquired_at = EXCLUDED.acquired_at,
                expires_at = EXCLUDED.expires_at,
                updated_at = NOW()
            WHERE rebuild_locks.expires_at <= NOW()
               OR rebuild_locks.owner_id = EXCLUDED.owner_id
            "#,
        )
        .bind(lock_name)
        .bind(owner_id)
        .bind(lease_end)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() > 0)
    }

    async fn renew_lock(
        &self,
        lock_name: &str,
        owner_id: Uuid,
        lease_duration: Duration,
    ) -> Result<bool, RebuildError> {
        let lease_end = Utc::now()
            + chrono::Duration::from_std(lease_duration)
                .map_err(|err| RebuildError::InvalidContract(err.to_string()))?;

        let result = sqlx::query(
            r#"
            UPDATE rebuild_locks
            SET expires_at = $3,
                updated_at = NOW()
            WHERE lock_name = $1
              AND owner_id = $2
            "#,
        )
        .bind(lock_name)
        .bind(owner_id)
        .bind(lease_end)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() > 0)
    }

    async fn release_lock(&self, lock_name: &str, owner_id: Uuid) -> Result<(), RebuildError> {
        sqlx::query(
            r#"
            DELETE FROM rebuild_locks
            WHERE lock_name = $1
              AND owner_id = $2
            "#,
        )
        .bind(lock_name)
        .bind(owner_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn current_graph_version(&self) -> Result<i64, RebuildError> {
        let (version,): (i64,) = sqlx::query_as(
            r#"
            SELECT graph_version
            FROM graph_state
            WHERE singleton = TRUE
            "#,
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(version)
    }

    async fn bump_graph_version(&self) -> Result<i64, RebuildError> {
        let (version,): (i64,) = sqlx::query_as(
            r#"
            UPDATE graph_state
            SET graph_version = graph_version + 1,
                updated_at = NOW()
            WHERE singleton = TRUE
            RETURNING graph_version
            "#,
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(version)
    }
}
