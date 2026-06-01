//! Durable transcript-ingest queue drain (todo 103, Option 4 — folds T07).
//!
//! Replaces T07's filesystem-scan `reconcile_transcripts()` with a level-
//! triggered queue drain: the `transcript_ingest_queue` row IS the marker and
//! the work item. Each `pending` row carries transcript *content* (captured on
//! the host by the `SessionEnd` / `PreCompact` command hooks), so the drain
//! feeds it to the extractor via `transcript_inline` and never touches the
//! absolute-path validator that the shipped `{{transcript_path}}` value tripped.
//!
//! Crash-safety: rows persist until marked `processed`, so a worker restart
//! resumes the backlog. Per-sweep work is bounded (`batch_limit`) so a large
//! post-downtime backlog drains across cycles without stampeding the extractor.

use infrastructure::{MAX_TRANSCRIPT_DRAIN_ATTEMPTS, TranscriptIngestQueue, TranscriptQueueError};
use session_extractor::{ExtractSessionRequest, SessionExtractor};
use thiserror::Error;
use tracing::{info, warn};

/// Default number of queue rows processed per drain sweep.
pub const DEFAULT_TRANSCRIPT_DRAIN_BATCH: i64 = 16;

/// One drain sweep's tally.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TranscriptDrainReport {
    pub claimed: usize,
    pub processed: usize,
    pub failed: usize,
}

#[derive(Debug, Error)]
pub enum TranscriptDrainError {
    #[error("transcript queue access failed: {0}")]
    Queue(#[from] TranscriptQueueError),
}

/// Drains queued transcripts into `.pending` drafts via the session extractor.
pub struct TranscriptQueueDrain {
    queue: TranscriptIngestQueue,
    extractor: SessionExtractor,
    batch_limit: i64,
}

impl TranscriptQueueDrain {
    /// Builds a drain over `queue`, extracting with `extractor`, processing at
    /// most `batch_limit` rows per sweep.
    pub fn new(queue: TranscriptIngestQueue, extractor: SessionExtractor, batch_limit: i64) -> Self {
        Self {
            queue,
            extractor,
            batch_limit: batch_limit.max(1),
        }
    }

    /// Claims and processes one bounded batch of pending transcripts.
    ///
    /// For each claimed row: feed its content to the extractor as
    /// `transcript_inline`; mark `processed` only on success (so a draft is
    /// durably written before the work is acked), or `failed`/retry on error.
    pub async fn drain_once(&self) -> Result<TranscriptDrainReport, TranscriptDrainError> {
        let claimed = self.queue.claim_pending(self.batch_limit).await?;
        let mut report = TranscriptDrainReport {
            claimed: claimed.len(),
            ..TranscriptDrainReport::default()
        };

        for row in claimed {
            let request = ExtractSessionRequest {
                // Inline content is authoritative; the ref is unused on this
                // path (validate_ref is skipped when transcript_inline is set).
                transcript_ref: String::new(),
                transcript_inline: Some(row.content),
                session_id: row.session_id.clone(),
                repo_path: row.repo_path.clone(),
            };

            match self.extractor.extract_blocking(&request).await {
                Ok(draft_paths) => {
                    self.queue.mark_processed(row.id).await?;
                    report.processed += 1;
                    info!(
                        row_id = %row.id,
                        session_id = %row.session_id,
                        source = row.source.as_str(),
                        draft_count = draft_paths.len(),
                        "transcript queue row drained to pending drafts"
                    );
                }
                Err(reason_code) => {
                    self.queue
                        .mark_failed(row.id, &reason_code, MAX_TRANSCRIPT_DRAIN_ATTEMPTS)
                        .await?;
                    report.failed += 1;
                    warn!(
                        row_id = %row.id,
                        session_id = %row.session_id,
                        source = row.source.as_str(),
                        attempts = row.attempts + 1,
                        reason_code = %reason_code,
                        "transcript queue row extraction failed; re-queued or parked"
                    );
                }
            }
        }

        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_default_is_zeroed() {
        let report = TranscriptDrainReport::default();
        assert_eq!(report.claimed, 0);
        assert_eq!(report.processed, 0);
        assert_eq!(report.failed, 0);
    }

    #[test]
    fn default_batch_is_positive() {
        assert!(DEFAULT_TRANSCRIPT_DRAIN_BATCH > 0);
    }
}
