//! Durable transcript-ingest queue (todo 103, Option 4).
//!
//! Host `command` hooks read a session transcript where its path is natively
//! valid and POST the *content* to the localhost ingest endpoint, which calls
//! [`TranscriptIngestQueue::enqueue`]. The maintenance worker drains `pending`
//! rows with [`TranscriptIngestQueue::claim_pending`] and feeds each row's
//! content to the extractor via `transcript_inline`, so the absolute
//! transcript-path validator is never exercised.
//!
//! This folds the planned T07 `processed_transcripts` marker into a single
//! content-bearing queue: the row IS both the work item and the dedup marker,
//! keyed on `content_hash`.

use chrono::{Duration, Utc};
use sqlx::{PgPool, Row};
use thiserror::Error;
use uuid::Uuid;

/// Maximum accepted transcript payload size, in bytes.
///
/// Mirrors `session_extractor::transcripts::MAX_INLINE_BYTES` so the ingest cap
/// and the extractor's inline cap agree; oversize payloads are rejected at
/// ingest time (todo 114) before a row is ever written.
pub const MAX_TRANSCRIPT_INGEST_BYTES: usize = 10 * 1024 * 1024;

/// Retry budget for a queued transcript before it is parked in `failed`.
pub const MAX_TRANSCRIPT_DRAIN_ATTEMPTS: i32 = 3;

/// Capture point that produced a queued transcript.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptSource {
    /// `SessionEnd` lifecycle hook.
    SessionEnd,
    /// `PreCompact` lifecycle hook (pre-summarization snapshot).
    PreCompact,
    /// Maintenance-side reconcile sweep (belt-and-suspenders backstop).
    Reconcile,
}

impl TranscriptSource {
    /// Returns the DB-canonical string for this source.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SessionEnd => "session_end",
            Self::PreCompact => "pre_compact",
            Self::Reconcile => "reconcile",
        }
    }

    /// Parses an ingest `source` value, rejecting unknown variants.
    pub fn parse(raw: &str) -> Result<Self, TranscriptQueueError> {
        match raw.trim() {
            "session_end" => Ok(Self::SessionEnd),
            "pre_compact" => Ok(Self::PreCompact),
            "reconcile" => Ok(Self::Reconcile),
            other => Err(TranscriptQueueError::InvalidContract(format!(
                "unsupported transcript source `{other}`"
            ))),
        }
    }
}

/// One transcript-ingest request before it is written to the queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptIngestRequest {
    pub session_id: String,
    pub repo_path: Option<String>,
    pub source: TranscriptSource,
    pub content: String,
}

/// Result of an idempotent enqueue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnqueueOutcome {
    /// A new row was inserted.
    Enqueued { id: Uuid, content_hash: String },
    /// An identical payload (same `content_hash`) was already queued; no-op.
    Duplicate { content_hash: String },
}

impl EnqueueOutcome {
    /// Returns the content hash regardless of branch.
    pub fn content_hash(&self) -> &str {
        match self {
            Self::Enqueued { content_hash, .. } | Self::Duplicate { content_hash } => content_hash,
        }
    }

    /// Returns `true` when a new row was written.
    pub fn is_new(&self) -> bool {
        matches!(self, Self::Enqueued { .. })
    }
}

/// A claimed queue row handed to a drain worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptQueueRecord {
    pub id: Uuid,
    pub session_id: String,
    pub content_hash: String,
    pub content: String,
    pub source: TranscriptSource,
    pub repo_path: Option<String>,
    pub attempts: i32,
}

#[derive(Debug, Error)]
pub enum TranscriptQueueError {
    #[error("transcript content must not be empty")]
    EmptyContent,
    #[error("transcript content exceeds {limit} bytes (got {actual})")]
    ContentTooLarge { limit: usize, actual: usize },
    #[error("invalid transcript ingest contract: {0}")]
    InvalidContract(String),
    #[error("transcript queue persistence error: {0}")]
    Persistence(#[from] sqlx::Error),
}

impl TranscriptQueueError {
    /// Stable reason code for API/log surfaces.
    pub fn reason_code(&self) -> &'static str {
        match self {
            Self::EmptyContent => "transcript_content_empty",
            Self::ContentTooLarge { .. } => "transcript_content_too_large",
            Self::InvalidContract(_) => "transcript_ingest_invalid_contract",
            Self::Persistence(_) => "transcript_queue_persistence_failed",
        }
    }
}

/// Postgres-backed transcript-ingest queue.
#[derive(Debug, Clone)]
pub struct TranscriptIngestQueue {
    pool: PgPool,
}

impl TranscriptIngestQueue {
    /// Wraps a connection pool. Assumes migration 002 has been applied.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Computes the canonical dedup hash for transcript content.
    ///
    /// blake3 hex, matching the digest family the outbox uses for Qdrant point
    /// ids — one hashing primitive across the system.
    pub fn content_hash(content: &str) -> String {
        blake3::hash(content.as_bytes()).to_hex().to_string()
    }

    /// Inserts a queued transcript, deduping on `content_hash`.
    ///
    /// Returns [`EnqueueOutcome::Duplicate`] (a no-op) when an identical payload
    /// is already queued, so a `SessionEnd` capture that repeats a `PreCompact`
    /// tail is idempotent. Enforces the non-empty and size-cap contracts before
    /// touching the database.
    pub async fn enqueue(
        &self,
        request: &TranscriptIngestRequest,
    ) -> Result<EnqueueOutcome, TranscriptQueueError> {
        if request.session_id.trim().is_empty() {
            return Err(TranscriptQueueError::InvalidContract(
                "session_id must not be blank".to_owned(),
            ));
        }
        if request.content.trim().is_empty() {
            return Err(TranscriptQueueError::EmptyContent);
        }
        if request.content.len() > MAX_TRANSCRIPT_INGEST_BYTES {
            return Err(TranscriptQueueError::ContentTooLarge {
                limit: MAX_TRANSCRIPT_INGEST_BYTES,
                actual: request.content.len(),
            });
        }

        let content_hash = Self::content_hash(&request.content);
        let id = Uuid::now_v7();
        let repo_path = request
            .repo_path
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());

        // ON CONFLICT (content_hash) DO NOTHING makes the insert idempotent;
        // RETURNING yields a row only when this call actually wrote one.
        let inserted_id: Option<Uuid> = sqlx::query(
            r#"
            INSERT INTO transcript_ingest_queue (
                id, session_id, content_hash, content, source, repo_path, status
            ) VALUES ($1, $2, $3, $4, $5, $6, 'pending')
            ON CONFLICT (content_hash) DO NOTHING
            RETURNING id
            "#,
        )
        .bind(id)
        .bind(&request.session_id)
        .bind(&content_hash)
        .bind(&request.content)
        .bind(request.source.as_str())
        .bind(repo_path)
        .fetch_optional(&self.pool)
        .await?
        .map(|row| row.get::<Uuid, _>("id"));

        Ok(match inserted_id {
            Some(id) => EnqueueOutcome::Enqueued { id, content_hash },
            None => EnqueueOutcome::Duplicate { content_hash },
        })
    }

    /// Claims up to `limit` pending rows, flipping them to `processing`.
    ///
    /// Uses `FOR UPDATE SKIP LOCKED` so concurrent drains never collide; bounds
    /// per-sweep work so a post-downtime backlog drains across cycles without
    /// stampeding the extractor.
    pub async fn claim_pending(
        &self,
        limit: i64,
    ) -> Result<Vec<TranscriptQueueRecord>, TranscriptQueueError> {
        if limit <= 0 {
            return Err(TranscriptQueueError::InvalidContract(
                "claim limit must be greater than zero".to_owned(),
            ));
        }

        let rows = sqlx::query(
            r#"
            UPDATE transcript_ingest_queue
            SET status = 'processing', updated_at = NOW()
            WHERE id IN (
                SELECT id
                FROM transcript_ingest_queue
                WHERE status = 'pending'
                ORDER BY enqueued_at ASC
                LIMIT $1
                FOR UPDATE SKIP LOCKED
            )
            RETURNING id, session_id, content_hash, content, source, repo_path, attempts
            "#,
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter()
            .map(|row| {
                Ok(TranscriptQueueRecord {
                    id: row.try_get("id")?,
                    session_id: row.try_get("session_id")?,
                    content_hash: row.try_get("content_hash")?,
                    content: row.try_get("content")?,
                    source: TranscriptSource::parse(row.try_get::<String, _>("source")?.as_str())?,
                    repo_path: row.try_get("repo_path")?,
                    attempts: row.try_get("attempts")?,
                })
            })
            .collect()
    }

    /// Marks a claimed row `processed`. Idempotent: a row already left
    /// `processing` (e.g. by a crash-recovered duplicate claim) is a no-op.
    pub async fn mark_processed(&self, id: Uuid) -> Result<(), TranscriptQueueError> {
        sqlx::query(
            r#"
            UPDATE transcript_ingest_queue
            SET status = 'processed', processed_at = NOW(), error = NULL, updated_at = NOW()
            WHERE id = $1 AND status = 'processing'
            "#,
        )
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Records a drain failure. Re-queues for retry until `max_attempts` is
    /// reached, after which the row is parked in `failed` with the last error.
    pub async fn mark_failed(
        &self,
        id: Uuid,
        error_message: &str,
        max_attempts: i32,
    ) -> Result<(), TranscriptQueueError> {
        sqlx::query(
            r#"
            UPDATE transcript_ingest_queue
            SET status = CASE
                    WHEN attempts + 1 >= $3 THEN 'failed'
                    ELSE 'pending'
                END,
                attempts = attempts + 1,
                error = $2,
                updated_at = NOW()
            WHERE id = $1 AND status = 'processing'
            "#,
        )
        .bind(id)
        .bind(error_message)
        .bind(max_attempts)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Returns the current status for a row, if present. Test/diagnostic helper.
    #[cfg(any(test, feature = "test-utils"))]
    pub async fn status_of(&self, id: Uuid) -> Result<Option<String>, TranscriptQueueError> {
        let row = sqlx::query("SELECT status FROM transcript_ingest_queue WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|row| row.get::<String, _>("status")))
    }

    /// Looks up a queued row by content hash. Test/diagnostic helper.
    #[cfg(any(test, feature = "test-utils"))]
    pub async fn find_status_by_hash(
        &self,
        content_hash: &str,
    ) -> Result<Option<String>, TranscriptQueueError> {
        let row = sqlx::query("SELECT status FROM transcript_ingest_queue WHERE content_hash = $1")
            .bind(content_hash)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|row| row.get::<String, _>("status")))
    }

    /// Counts rows in a given status. Test/diagnostic helper.
    #[cfg(any(test, feature = "test-utils"))]
    pub async fn count_with_status(&self, status: &str) -> Result<i64, TranscriptQueueError> {
        let (count,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM transcript_ingest_queue WHERE status = $1")
                .bind(status)
                .fetch_one(&self.pool)
                .await?;
        Ok(count)
    }
}

/// Computes a retry timestamp `retry_after` seconds in the future.
///
/// Exposed for symmetry with the outbox's scheduling helper; the queue itself
/// retries by flipping back to `pending` (claimed again on the next sweep), so
/// callers that want a delayed retry can persist this alongside.
pub fn transcript_retry_at(retry_after_seconds: i64) -> chrono::DateTime<Utc> {
    Utc::now() + Duration::seconds(retry_after_seconds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_hash_is_deterministic_and_distinct() {
        let a = TranscriptIngestQueue::content_hash("hello world");
        let b = TranscriptIngestQueue::content_hash("hello world");
        let c = TranscriptIngestQueue::content_hash("different");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(a.len(), 64, "blake3 hex is 64 chars");
    }

    #[test]
    fn source_round_trips_through_db_string() {
        for source in [
            TranscriptSource::SessionEnd,
            TranscriptSource::PreCompact,
            TranscriptSource::Reconcile,
        ] {
            assert_eq!(TranscriptSource::parse(source.as_str()).unwrap(), source);
        }
    }

    #[test]
    fn source_parse_rejects_unknown() {
        let error = TranscriptSource::parse("bogus").expect_err("unknown source rejected");
        assert!(matches!(error, TranscriptQueueError::InvalidContract(_)));
    }

    #[test]
    fn enqueue_outcome_exposes_hash_on_both_branches() {
        let enqueued = EnqueueOutcome::Enqueued {
            id: Uuid::now_v7(),
            content_hash: "abc".to_owned(),
        };
        let duplicate = EnqueueOutcome::Duplicate {
            content_hash: "abc".to_owned(),
        };
        assert_eq!(enqueued.content_hash(), "abc");
        assert_eq!(duplicate.content_hash(), "abc");
        assert!(enqueued.is_new());
        assert!(!duplicate.is_new());
    }

    #[test]
    fn error_reason_codes_are_stable() {
        assert_eq!(
            TranscriptQueueError::EmptyContent.reason_code(),
            "transcript_content_empty"
        );
        assert_eq!(
            TranscriptQueueError::ContentTooLarge {
                limit: 1,
                actual: 2
            }
            .reason_code(),
            "transcript_content_too_large"
        );
    }
}
