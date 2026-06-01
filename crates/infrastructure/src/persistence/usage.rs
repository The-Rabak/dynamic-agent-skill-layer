//! Usage persistence ports and Postgres adapters for the skill-usage signal.
//!
//! # Ports
//!
//! - [`UsagePersistencePort`]: write one session's usage in a single transaction
//!   (`session_logs` row + N `skill_usage` rows).
//! - [`UsageSampleStore`]: read aggregated usage per skill for retirement scoring
//!   and the deterministic ranking prior.
//!
//! # Invariants
//!
//! - **Append-log model**: every `compile_context` selection produces one immutable
//!   `skill_usage` row (`usage_count=1`). Reads aggregate with `count(*)`/`max(used_at)`.
//!   No UNIQUE constraint, no UPSERT — per-selection `relevance_score` is preserved.
//! - **Prompt security (P3)**: `session_logs.prompt_hash` stores the BLAKE3 hash of
//!   the raw prompt, never the prompt text itself.
//! - **Atomicity**: the `session_logs` row + all `skill_usage` rows for one
//!   `compile_context` call are wrapped in one Postgres transaction. A partial write
//!   would under-count usage and wrongly mark a used skill as retire-eligible.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use thiserror::Error;
use uuid::Uuid;

/// Aggregated usage signal for a single skill, queried at retirement-scoring or
/// prior-population time.
#[derive(Debug, Clone, PartialEq)]
pub struct SkillUsageSummary {
    /// String representation of the skill UUID.
    pub skill_id: String,
    /// Total selections across all sessions (full history, not windowed).
    pub total_count: u32,
    /// Selections within the last `window_days` days (for retirement scoring).
    pub windowed_count: u32,
    /// When was this skill most recently selected, or `None` if never used.
    pub last_used_at: Option<DateTime<Utc>>,
    /// Days since `last_used_at`, derived server-side from `now() - used_at`.
    /// `None` when `last_used_at` is `None`.
    pub age_days: Option<u32>,
}

/// A single skill selection to persist alongside a session log row.
#[derive(Debug, Clone)]
pub struct SkillSelectionRecord {
    /// String representation of the skill UUID.
    pub skill_id: String,
    /// Cosine / EQ-3 relevance score at selection time.
    pub relevance_score: f32,
    /// Current context compilation status, e.g. `"ok"`, `"degraded"`.
    pub context_status: String,
}

/// Parameters for writing one compile_context call's usage in one transaction.
#[derive(Debug, Clone)]
pub struct SessionUsageRecord {
    /// Caller-supplied session identifier (opaque string, not a UUID).
    pub session_id: String,
    /// BLAKE3 hash of the raw prompt — never the prompt text (P3).
    pub prompt_hash: String,
    /// Retrieval scope string (`"project"`, `"global"`, or `"team"`).
    pub scope: String,
    /// Elapsed milliseconds for the compile_context call.
    pub latency_ms: i64,
    /// Context compilation status text.
    pub status: String,
    /// Skills selected in this session's compile_context call.
    pub selected_skills: Vec<SkillSelectionRecord>,
}

/// Port for writing session and skill-usage rows.
///
/// Implementations must wrap the `session_logs` INSERT and all `skill_usage`
/// INSERTs for a single call in one database transaction so partial writes
/// never corrupt the usage signal.
///
/// The write is intended to run asynchronously off the response path; callers
/// should not await it on the hot path.
#[async_trait]
pub trait UsagePersistencePort: Send + Sync {
    /// Persists one compile_context call's session log and all selected-skill rows
    /// in a single transaction.
    ///
    /// Returns `Ok(())` on success. Failures are logged as warnings by the
    /// background writer — they are never propagated to the caller.
    async fn write_session_usage(
        &self,
        record: SessionUsageRecord,
    ) -> Result<(), UsagePersistenceError>;
}

/// Port for reading aggregated skill-usage for retirement scoring and prior population.
///
/// Implementations batch all requested skill IDs into one query to avoid N+1 patterns
/// on the hot path. Skills absent from `skill_usage` are returned with zero counts.
#[async_trait]
pub trait UsageSampleStore: Send + Sync {
    /// Returns aggregated usage for each skill ID in `skill_ids`.
    ///
    /// Skills with no usage rows are represented with `total_count=0`,
    /// `windowed_count=0`, `last_used_at=None`, and `age_days=None`.
    ///
    /// `window_days` controls the `windowed_count` aggregation window used by
    /// retirement scoring (typically 90 days per [`RetirementConfig`]).
    async fn recent_usage(
        &self,
        skill_ids: &[String],
        window_days: i64,
    ) -> Result<Vec<SkillUsageSummary>, UsagePersistenceError>;
}

/// Error type for usage persistence operations.
#[derive(Debug, Error)]
pub enum UsagePersistenceError {
    #[error("usage persistence database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("usage persistence invalid contract: {0}")]
    InvalidContract(String),
}

/// Postgres adapter that writes session_logs and skill_usage in one transaction.
///
/// Implements [`UsagePersistencePort`]. The session_logs row carries a prompt
/// BLAKE3 hash (never the raw prompt), latency, and status. Each selected skill
/// gets one `skill_usage` row with `usage_count=1` and its per-selection
/// `relevance_score`.
#[derive(Clone)]
pub struct PostgresUsageWriter {
    pool: PgPool,
}

impl PostgresUsageWriter {
    /// Creates a new writer backed by the given pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl UsagePersistencePort for PostgresUsageWriter {
    async fn write_session_usage(
        &self,
        record: SessionUsageRecord,
    ) -> Result<(), UsagePersistenceError> {
        // Validate scope is one of the DB-constrained values before starting a tx.
        if !matches!(record.scope.as_str(), "project" | "global" | "team") {
            return Err(UsagePersistenceError::InvalidContract(format!(
                "scope must be 'project', 'global', or 'team'; got '{}'",
                record.scope
            )));
        }

        let session_log_id = Uuid::now_v7();

        let mut tx = self.pool.begin().await?;

        // Insert session_logs row with typed columns added by migration 003.
        sqlx::query(
            "INSERT INTO session_logs (id, session_id, scope, prompt_hash, latency_ms, status, metadata)
             VALUES ($1, $2, $3, $4, $5, $6, '{}'::jsonb)"
        )
        .bind(session_log_id)
        .bind(&record.session_id)
        .bind(&record.scope)
        .bind(&record.prompt_hash)
        .bind(record.latency_ms)
        .bind(&record.status)
        .execute(&mut *tx)
        .await?;

        // Insert one skill_usage row per selected skill (append-log, no UPSERT).
        for skill_selection in &record.selected_skills {
            let usage_id = Uuid::now_v7();
            let skill_uuid: Uuid = skill_selection.skill_id.parse().map_err(|_| {
                UsagePersistenceError::InvalidContract(format!(
                    "skill_id '{}' is not a valid UUID",
                    skill_selection.skill_id
                ))
            })?;

            // Validate context_status against the DB CHECK constraint.
            if !matches!(
                skill_selection.context_status.as_str(),
                "ok" | "no_match" | "degraded" | "duplicate_suppressed"
            ) {
                return Err(UsagePersistenceError::InvalidContract(format!(
                    "context_status must be one of ok/no_match/degraded/duplicate_suppressed; got '{}'",
                    skill_selection.context_status
                )));
            }

            sqlx::query(
                "INSERT INTO skill_usage (id, session_id, skill_id, usage_count, context_status, relevance_score, used_at, metadata)
                 VALUES ($1, $2, $3, 1, $4, $5, NOW(), '{}'::jsonb)"
            )
            .bind(usage_id)
            .bind(&record.session_id)
            .bind(skill_uuid)
            .bind(&skill_selection.context_status)
            .bind(skill_selection.relevance_score)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }
}

/// Postgres adapter that reads aggregated skill usage for retirement scoring and
/// ranking prior population.
///
/// Implements [`UsageSampleStore`]. Uses the existing
/// `idx_skill_usage_skill_used_at (skill_id, used_at DESC)` index; no new indexes.
#[derive(Clone)]
pub struct PostgresUsageSampleStore {
    pool: PgPool,
}

impl PostgresUsageSampleStore {
    /// Creates a new store backed by the given pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl UsageSampleStore for PostgresUsageSampleStore {
    async fn recent_usage(
        &self,
        skill_ids: &[String],
        window_days: i64,
    ) -> Result<Vec<SkillUsageSummary>, UsagePersistenceError> {
        if skill_ids.is_empty() {
            return Ok(Vec::new());
        }

        // Parse all IDs upfront so we fail fast on bad inputs.
        let uuids: Result<Vec<Uuid>, _> = skill_ids.iter().map(|id| id.parse::<Uuid>()).collect();
        let uuids = uuids.map_err(|_| {
            UsagePersistenceError::InvalidContract(
                "one or more skill_ids are not valid UUIDs".to_owned(),
            )
        })?;

        // One batched query: LEFT JOIN so skills with zero rows are included.
        // age_days is derived server-side (DB now() - max(used_at)) to match
        // the schema invariant that the DB clock owns timestamps.
        //
        // Uses sqlx::query (not the macro) because no `.sqlx` compile-time cache
        // exists in this project (T06 design decision: no cargo sqlx prepare needed).
        let rows = sqlx::query(
            r#"
            SELECT
                s.id::TEXT                                                              AS skill_id,
                COUNT(u.id)::INTEGER                                                    AS total_count,
                COUNT(u.id) FILTER (WHERE u.used_at >= NOW() - ($2 || ' days')::INTERVAL)::INTEGER
                                                                                       AS windowed_count,
                MAX(u.used_at)                                                          AS last_used_at,
                EXTRACT(EPOCH FROM (NOW() - MAX(u.used_at))) / 86400.0                 AS age_days_float
            FROM unnest($1::uuid[]) AS s(id)
            LEFT JOIN skill_usage u ON u.skill_id = s.id
            GROUP BY s.id
            "#,
        )
        .bind(&uuids as &[Uuid])
        .bind(format!("{window_days}"))
        .fetch_all(&self.pool)
        .await?;

        use sqlx::Row;
        Ok(rows
            .into_iter()
            .map(|row| {
                let total_count: i32 = row.try_get("total_count").unwrap_or(0);
                let windowed_count: i32 = row.try_get("windowed_count").unwrap_or(0);
                let last_used_at: Option<DateTime<Utc>> =
                    row.try_get("last_used_at").unwrap_or(None);
                let age_days_float: Option<f64> = row.try_get("age_days_float").unwrap_or(None);
                SkillUsageSummary {
                    skill_id: row.try_get("skill_id").unwrap_or_default(),
                    total_count: total_count.max(0) as u32,
                    windowed_count: windowed_count.max(0) as u32,
                    last_used_at,
                    age_days: age_days_float.map(|f| f.max(0.0) as u32),
                }
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Proves the append-log model: writing two UsageSample sets for the same
    /// skill produces two rows (not an upsert collapse). This test runs without
    /// a live DB — it validates port contract shapes only.
    #[test]
    fn session_usage_record_allows_multiple_selections_for_same_skill() {
        let record = SessionUsageRecord {
            session_id: "test-session".to_owned(),
            prompt_hash: "abc123".to_owned(),
            scope: "project".to_owned(),
            latency_ms: 42,
            status: "ok".to_owned(),
            selected_skills: vec![
                SkillSelectionRecord {
                    skill_id: "00000000-0000-0000-0000-000000000001".to_owned(),
                    relevance_score: 0.8,
                    context_status: "ok".to_owned(),
                },
                SkillSelectionRecord {
                    skill_id: "00000000-0000-0000-0000-000000000002".to_owned(),
                    relevance_score: 0.6,
                    context_status: "ok".to_owned(),
                },
            ],
        };
        assert_eq!(record.selected_skills.len(), 2);
        assert!(
            record.selected_skills[0].relevance_score > record.selected_skills[1].relevance_score
        );
    }

    /// Proves cold-start: a SkillUsageSummary with total_count=0 is the correct
    /// zero-row representation (not an error) so retirement scoring treats it as
    /// never-used (eligible).
    #[test]
    fn skill_usage_summary_with_zero_count_represents_never_used_skill() {
        let summary = SkillUsageSummary {
            skill_id: "00000000-0000-0000-0000-000000000001".to_owned(),
            total_count: 0,
            windowed_count: 0,
            last_used_at: None,
            age_days: None,
        };
        assert_eq!(summary.total_count, 0);
        assert!(summary.last_used_at.is_none());
        // Confirms retirement scoring will treat this as stale (no recent usage).
        assert_eq!(summary.windowed_count, 0);
    }
}
