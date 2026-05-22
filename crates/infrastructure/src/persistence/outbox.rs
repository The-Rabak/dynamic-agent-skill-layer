use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use sqlx::{PgPool, Row};
use thiserror::Error;
use uuid::Uuid;

use crate::streaming::redis::EventEnvelope;

#[derive(Debug, Clone, PartialEq)]
pub struct OutboxRecord {
    pub envelope: EventEnvelope,
    pub attempts: i32,
    pub stream_id: Option<String>,
    pub last_error: Option<String>,
    pub occurred_at: DateTime<Utc>,
    pub available_at: DateTime<Utc>,
}

pub const MAX_OUTBOX_RETRIES: i32 = 3;

#[derive(Debug, Error)]
pub enum OutboxError {
    #[error("invalid outbox contract: {0}")]
    InvalidContract(String),
    #[error("outbox idempotency conflict for key `{idempotency_key}`")]
    IdempotencyConflict { idempotency_key: String },
    #[error("outbox event `{event_id}` was not found")]
    EventNotFound { event_id: Uuid },
    #[error("outbox transition requires `{expected}` state for `{event_id}`, got `{actual}`")]
    IllegalTransition {
        event_id: Uuid,
        expected: &'static str,
        actual: String,
    },
    #[error("outbox persistence error: {0}")]
    Persistence(#[from] sqlx::Error),
}

#[async_trait]
pub trait GraphWriteCoordinator: Send + Sync {
    async fn append_outbox_event(&self, envelope: &EventEnvelope) -> Result<(), OutboxError>;
    async fn claim_pending_outbox(&self, limit: i64) -> Result<Vec<OutboxRecord>, OutboxError>;
    async fn mark_outbox_published(
        &self,
        event_id: Uuid,
        stream_id: &str,
    ) -> Result<(), OutboxError>;
    async fn mark_outbox_failed(
        &self,
        event_id: Uuid,
        error_message: &str,
        retry_after_seconds: u64,
    ) -> Result<(), OutboxError>;
}

#[derive(Debug, Clone)]
pub struct PostgresGraphWriteCoordinator {
    pool: PgPool,
}

impl PostgresGraphWriteCoordinator {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    async fn transition_error(
        &self,
        event_id: Uuid,
        expected: &'static str,
    ) -> Result<OutboxError, OutboxError> {
        let maybe_row = sqlx::query(
            r#"
            SELECT status
            FROM outbox_events
            WHERE event_id = $1
            "#,
        )
        .bind(event_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(match maybe_row {
            Some(row) => OutboxError::IllegalTransition {
                event_id,
                expected,
                actual: row.try_get("status")?,
            },
            None => OutboxError::EventNotFound { event_id },
        })
    }
}

#[async_trait]
impl GraphWriteCoordinator for PostgresGraphWriteCoordinator {
    async fn append_outbox_event(&self, envelope: &EventEnvelope) -> Result<(), OutboxError> {
        if envelope.event_type.trim().is_empty() || envelope.idempotency_key.trim().is_empty() {
            return Err(OutboxError::InvalidContract(
                "event_type and idempotency_key must not be blank".to_owned(),
            ));
        }

        let schema_version = i32::try_from(envelope.schema_version).map_err(|_| {
            OutboxError::InvalidContract("schema_version exceeds i32 storage limit".to_owned())
        })?;

        let insert_result = sqlx::query(
            r#"
            INSERT INTO outbox_events (
                event_id,
                event_type,
                correlation_id,
                idempotency_key,
                schema_version,
                payload,
                occurred_at,
                available_at,
                status
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, NOW(), 'pending')
            "#,
        )
        .bind(envelope.event_id)
        .bind(&envelope.event_type)
        .bind(envelope.correlation_id)
        .bind(&envelope.idempotency_key)
        .bind(schema_version)
        .bind(&envelope.payload)
        .bind(envelope.timestamp)
        .execute(&self.pool)
        .await;

        match insert_result {
            Ok(_) => Ok(()),
            Err(sqlx::Error::Database(error)) if error.is_unique_violation() => {
                let constraint = error.constraint();
                if constraint == Some("outbox_events_idempotency_key_key") {
                    return Err(OutboxError::IdempotencyConflict {
                        idempotency_key: envelope.idempotency_key.clone(),
                    });
                }

                Err(OutboxError::InvalidContract(format!(
                    "outbox event already exists for event_id {}",
                    envelope.event_id
                )))
            }
            Err(error) => Err(OutboxError::Persistence(error)),
        }
    }

    async fn claim_pending_outbox(&self, limit: i64) -> Result<Vec<OutboxRecord>, OutboxError> {
        if limit <= 0 {
            return Err(OutboxError::InvalidContract(
                "claim limit must be greater than zero".to_owned(),
            ));
        }

        let rows = sqlx::query(
            r#"
            UPDATE outbox_events
            SET status = 'processing', updated_at = NOW()
            WHERE event_id IN (
                SELECT event_id
                FROM outbox_events
                WHERE status = 'pending' AND available_at <= NOW()
                ORDER BY occurred_at ASC
                LIMIT $1
                FOR UPDATE SKIP LOCKED
            )
            RETURNING
                event_id,
                event_type,
                correlation_id,
                idempotency_key,
                schema_version,
                payload,
                attempts,
                stream_id,
                last_error,
                occurred_at,
                available_at
            "#,
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        let mut records = Vec::with_capacity(rows.len());
        for row in rows {
            records.push(OutboxRecord {
                envelope: EventEnvelope {
                    event_id: row.try_get("event_id")?,
                    event_type: row.try_get("event_type")?,
                    correlation_id: row.try_get("correlation_id")?,
                    idempotency_key: row.try_get("idempotency_key")?,
                    schema_version: row.try_get::<i32, _>("schema_version")? as u32,
                    timestamp: row.try_get("occurred_at")?,
                    payload: row.try_get("payload")?,
                },
                attempts: row.try_get("attempts")?,
                stream_id: row.try_get("stream_id")?,
                last_error: row.try_get("last_error")?,
                occurred_at: row.try_get("occurred_at")?,
                available_at: row.try_get("available_at")?,
            });
        }

        Ok(records)
    }

    async fn mark_outbox_published(
        &self,
        event_id: Uuid,
        stream_id: &str,
    ) -> Result<(), OutboxError> {
        if stream_id.trim().is_empty() {
            return Err(OutboxError::InvalidContract(
                "stream_id must not be blank".to_owned(),
            ));
        }

        let update_result = sqlx::query(
            r#"
            UPDATE outbox_events
            SET status = 'published',
                stream_id = $2,
                published_at = NOW(),
                updated_at = NOW()
            WHERE event_id = $1
              AND status = 'processing'
            "#,
        )
        .bind(event_id)
        .bind(stream_id)
        .execute(&self.pool)
        .await?;

        if update_result.rows_affected() == 1 {
            return Ok(());
        }

        Err(self.transition_error(event_id, "processing").await?)
    }

    async fn mark_outbox_failed(
        &self,
        event_id: Uuid,
        error_message: &str,
        retry_after_seconds: u64,
    ) -> Result<(), OutboxError> {
        if error_message.trim().is_empty() {
            return Err(OutboxError::InvalidContract(
                "error_message must not be blank".to_owned(),
            ));
        }

        let retry_seconds = i64::try_from(retry_after_seconds).map_err(|_| {
            OutboxError::InvalidContract(
                "retry_after_seconds exceeds i64 range for scheduling".to_owned(),
            )
        })?;
        let available_at = Utc::now() + Duration::seconds(retry_seconds);

        let update_result = sqlx::query(
            r#"
            UPDATE outbox_events
            SET status = CASE
                    WHEN attempts + 1 >= $4 THEN 'failed'
                    ELSE 'pending'
                END,
                attempts = attempts + 1,
                last_error = $2,
                available_at = CASE
                    WHEN attempts + 1 >= $4 THEN available_at
                    ELSE $3
                END,
                updated_at = NOW()
            WHERE event_id = $1
              AND status = 'processing'
            "#,
        )
        .bind(event_id)
        .bind(error_message)
        .bind(available_at)
        .bind(MAX_OUTBOX_RETRIES)
        .execute(&self.pool)
        .await?;

        if update_result.rows_affected() == 1 {
            return Ok(());
        }

        Err(self.transition_error(event_id, "processing").await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outbox_retry_limit_matches_event_contract() {
        assert_eq!(MAX_OUTBOX_RETRIES, 3);
    }
}
