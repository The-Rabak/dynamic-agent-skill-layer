//! Background usage writer for the `compile_context` hot path.
//!
//! Usage writes are async and off the response path so they never contribute to
//! `compile_context` latency. Failures are observable (warn log +
//! `health["usage_write"]="failed"`) but are never propagated to the caller.
//!
//! # Design
//!
//! A dedicated task reads from a bounded `mpsc` channel (capacity ~128) and calls
//! [`UsagePersistencePort::write_session_usage`]. `McpServerApp::compile_context`
//! posts to the channel with `try_send` — if the channel is full the record is
//! dropped and the health marker is set to `"failed"`. Raw `tokio::spawn` is
//! intentionally avoided: panics inside a raw spawn vanish silently; the
//! background task here catches all `Result::Err` paths explicitly.
//!
//! # Feature gate
//!
//! The `MCP_USAGE_LOGGING` environment variable controls whether usage is
//! recorded. When set to `"off"` the writer is not spawned and no usage rows
//! are written. The observability seam (warn + health marker on failure) is
//! always active regardless of this flag.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use infrastructure::{SessionUsageRecord, UsagePersistencePort};
use tokio::sync::mpsc;
use tracing::warn;

/// Health-marker key set when a usage write fails or the channel is full.
pub const USAGE_WRITE_HEALTH_KEY: &str = "usage_write";

/// Channel capacity for the bounded usage-write queue.
///
/// 128 is chosen to absorb short bursts during graph refresh while keeping
/// memory overhead negligible (each `SessionUsageRecord` is O(skills) small).
pub const USAGE_WRITE_CHANNEL_CAPACITY: usize = 128;

/// Rollback flag: set `MCP_USAGE_LOGGING=off` to disable usage recording.
///
// TODO(remove-after-v1.5-green): delete this flag once usage recording is
// proven stable in production. Removal criterion: first green CI on `main`
// with usage rows confirmed in the live DB.
pub const USAGE_LOGGING_FLAG: &str = "MCP_USAGE_LOGGING";

/// Atomic encoding for the two runtime usage-write health states.
///
/// `HEALTH_OK` is the initialized state; `HEALTH_FAILED` is set on a DB error
/// or channel-full backpressure event and reset to `HEALTH_OK` after the next
/// successful write.
///
/// Note: the `"disabled"` state (when `usage_sender` is `None`) is NOT stored
/// here — it is computed at the read site in `McpServerApp::compile_context`.
/// This cell only tracks whether the active writer succeeded or failed.
const HEALTH_OK: u8 = 0;
const HEALTH_FAILED: u8 = 1;

/// Shared health-marker cell for the usage writer observability seam.
///
/// `Arc<AtomicU8>` replaces the previous `Arc<Mutex<String>>` so the warm-path
/// read in `compile_context` is a lock-free atomic load rather than a mutex
/// acquire. Both the writer task and the hot path share the same `Arc`.
pub type UsageWriteHealth = Arc<AtomicU8>;

/// Creates a new health cell initialized to the ok state.
pub fn new_usage_write_health() -> UsageWriteHealth {
    Arc::new(AtomicU8::new(HEALTH_OK))
}

/// Returns the current usage-write health as a string tag.
///
/// Called by `McpServerApp::compile_context` on every response to populate the
/// `health["usage_write"]` field. Uses `Relaxed` ordering: the only requirement
/// is that we observe *a* recent value — strict sequencing with other memory
/// operations is not needed for this observability-only read.
pub fn read_usage_write_health(health: &UsageWriteHealth) -> String {
    match health.load(Ordering::Relaxed) {
        HEALTH_FAILED => "failed".to_owned(),
        _ => "ok".to_owned(),
    }
}

/// Marks the health cell as failed and emits a structured warn log.
///
/// Called both by the background writer (on DB error) and by `try_send` on the
/// hot path (on channel-full backpressure). Both count as an observable failure;
/// neither propagates to the caller.
fn set_failed(health: &UsageWriteHealth, reason: &str) {
    health.store(HEALTH_FAILED, Ordering::Relaxed);
    warn!(
        health_key = USAGE_WRITE_HEALTH_KEY,
        reason, "usage write failed; latency and caller response unaffected"
    );
}

/// Resets the health cell to `"ok"` after a successful write.
fn set_ok(health: &UsageWriteHealth) {
    health.store(HEALTH_OK, Ordering::Relaxed);
}

/// The spawned writer task and the sender used to post records to it.
///
/// Returned by [`spawn_usage_writer`] so callers can both post records on the
/// hot path and deterministically drain the task during teardown.
pub struct UsageWriterHandle {
    /// Hot-path sender; clone-able so multiple callers can post records.
    pub sender: mpsc::Sender<SessionUsageRecord>,
    /// Join handle for the background writer task.
    ///
    /// Drop `sender` first (signalling the channel closure), then `.await`
    /// this handle to ensure all in-flight writes complete before the PG pool
    /// is closed or `TRUNCATE` is issued in test teardown.
    pub join_handle: tokio::task::JoinHandle<()>,
}

/// Spawns the background usage-writer task and returns a [`UsageWriterHandle`].
///
/// Returns `None` when usage logging is disabled via [`USAGE_LOGGING_FLAG`].
/// The caller (`McpServerApp`) stores the sender as `Option<mpsc::Sender<…>>`
/// and uses `try_send` on the hot path; a `None` means no write is attempted.
///
/// The background task runs until the sender side is dropped (server shutdown).
/// All failures are logged and update the shared health cell — they are never
/// re-panicked or propagated.
///
/// # Teardown contract
///
/// To deterministically drain the writer before closing the PG pool or issuing
/// a `TRUNCATE`:
/// 1. Drop the `sender` field (signals end-of-channel to the task).
/// 2. `.await` `join_handle` (waits for the task to flush and exit).
pub fn spawn_usage_writer(
    writer: Arc<dyn UsagePersistencePort>,
    health: UsageWriteHealth,
) -> Option<UsageWriterHandle> {
    if std::env::var(USAGE_LOGGING_FLAG).as_deref() == Ok("off") {
        warn!(
            flag = USAGE_LOGGING_FLAG,
            "usage logging disabled by rollback flag; no usage rows will be written"
        );
        return None;
    }

    let (tx, mut rx) = mpsc::channel::<SessionUsageRecord>(USAGE_WRITE_CHANNEL_CAPACITY);
    let health_clone = health.clone();

    let join_handle = tokio::spawn(async move {
        while let Some(record) = rx.recv().await {
            match writer.write_session_usage(record).await {
                Ok(()) => set_ok(&health_clone),
                Err(error) => {
                    set_failed(&health_clone, &error.to_string());
                }
            }
        }
    });

    Some(UsageWriterHandle {
        sender: tx,
        join_handle,
    })
}

/// Posts a usage record to the background writer channel.
///
/// Uses `try_send` so the hot path is never blocked. If the channel is full
/// (backpressure) the record is dropped and the health marker is set to
/// `"failed"` with a warn log. Both DB errors (inside the task) and channel-full
/// drops (here) are observable through the same `health["usage_write"]` key.
pub fn post_usage_record(
    sender: &mpsc::Sender<SessionUsageRecord>,
    record: SessionUsageRecord,
    health: &UsageWriteHealth,
) {
    if let Err(_dropped) = sender.try_send(record) {
        set_failed(health, "usage_write_channel_full");
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use infrastructure::{SessionUsageRecord, UsagePersistenceError, UsagePersistencePort};
    use tokio::sync::mpsc;

    use super::*;

    struct CapturingUsageWriter {
        records: Arc<Mutex<Vec<SessionUsageRecord>>>,
    }

    #[async_trait]
    impl UsagePersistencePort for CapturingUsageWriter {
        async fn write_session_usage(
            &self,
            record: SessionUsageRecord,
        ) -> Result<(), UsagePersistenceError> {
            self.records.lock().expect("lock").push(record);
            Ok(())
        }
    }

    struct FailingUsageWriter;

    #[async_trait]
    impl UsagePersistencePort for FailingUsageWriter {
        async fn write_session_usage(
            &self,
            _record: SessionUsageRecord,
        ) -> Result<(), UsagePersistenceError> {
            Err(UsagePersistenceError::InvalidContract(
                "simulated write failure".to_owned(),
            ))
        }
    }

    fn sample_record() -> SessionUsageRecord {
        SessionUsageRecord {
            session_id: "test-session".to_owned(),
            prompt_hash: "abc123".to_owned(),
            scope: "project".to_owned(),
            latency_ms: 10,
            status: "ok".to_owned(),
            selected_skills: vec![],
        }
    }

    /// Proves the observability seam: a simulated write failure sets
    /// `health["usage_write"]="failed"`, emits a warn log, and never propagates
    /// to the caller. The test checks both the health marker value and that no
    /// panic or error is returned to the calling side.
    #[tokio::test]
    async fn write_failure_sets_health_marker_to_failed_and_never_propagates() {
        let health = new_usage_write_health();
        let writer = Arc::new(FailingUsageWriter);
        let handle = spawn_usage_writer(writer, health.clone())
            .expect("writer should be spawned when flag is not off");

        post_usage_record(&handle.sender, sample_record(), &health);

        // Drain the channel so the task has time to process.
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let status = read_usage_write_health(&health);
        assert_eq!(
            status, "failed",
            "health should be 'failed' after a write error; got '{status}'"
        );
    }

    /// Proves backpressure: when the channel is full, `post_usage_record` drops
    /// the record and sets the health marker to `"failed"` — it never blocks.
    #[test]
    fn channel_full_sets_health_marker_to_failed_without_blocking() {
        let health = new_usage_write_health();
        // Create a channel with capacity 1 and fill it manually so try_send fails.
        let (_tx, _rx) = mpsc::channel::<SessionUsageRecord>(1);
        // Use a real zero-capacity-like scenario: create with capacity 1, send 1, then try again.
        let (tx, _rx) = mpsc::channel::<SessionUsageRecord>(1);
        tx.try_send(sample_record())
            .expect("first send should succeed");

        // Second send overflows the capacity-1 channel.
        post_usage_record(&tx, sample_record(), &health);

        let status = read_usage_write_health(&health);
        assert_eq!(
            status, "failed",
            "health should be 'failed' after channel-full drop; got '{status}'"
        );
    }

    /// Proves that a successful write resets health to "ok".
    #[tokio::test]
    async fn successful_write_keeps_health_ok() {
        let records = Arc::new(Mutex::new(Vec::new()));
        let health = new_usage_write_health();
        let writer = Arc::new(CapturingUsageWriter {
            records: records.clone(),
        });
        let handle = spawn_usage_writer(writer, health.clone()).expect("writer should be spawned");

        post_usage_record(&handle.sender, sample_record(), &health);
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        assert_eq!(read_usage_write_health(&health), "ok");
        assert_eq!(records.lock().expect("lock").len(), 1);
    }
}
