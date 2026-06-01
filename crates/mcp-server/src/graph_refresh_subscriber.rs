//! Online graph-refresh subscriber: keeps the in-memory CQRS read model fresh
//! without a server restart (T02, SC-V1.5-A online half).
//!
//! graph-builder publishes `graph.rebuilt` to the shared Redis stream after each
//! rebuild. This subscriber consumes those events via a Redis consumer group,
//! reloads the bounded Postgres snapshot, and atomically swaps the retriever's
//! graph handle. All Redis/PG knowledge lives here in `mcp-server`; `retrieval`
//! only exposes `RetrievalOrchestrator::swap_graph` (seam discipline, ADR-0001).
//!
//! Reliability contract:
//! - Reuses [`RedisStreamsAdapter::read_group`] (XREADGROUP: pending-replay `"0"`
//!   then new `">"`).
//! - ACKs a message only AFTER the reload+swap succeeds, so a crash mid-reload
//!   replays the event rather than dropping a rebuild.
//! - Coalesces a burst of `graph.rebuilt`: a single batch reload reflects the
//!   newest graph version; reloads are idempotent (re-applying the same version
//!   is a no-op via `swap_graph`).
//! - Wrapped in an exponential-backoff reconnect loop; it never panics and never
//!   blocks the HTTP server (it runs on its own spawned task).

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use infrastructure::{RedisStreamsAdapter, StreamMessage};
use tracing::{error, info, warn};

/// The frozen event (8-event catalog) that signals a completed graph rebuild.
const GRAPH_REBUILT_EVENT_TYPE: &str = "graph.rebuilt";

/// Number of stream messages to pull per `read_group` call. A rebuild burst is
/// coalesced into one reload regardless of how many land in a batch.
const READ_BATCH_SIZE: usize = 64;

/// How long `read_group` blocks waiting for new events before looping. Keeps the
/// task responsive to shutdown and reconnect without busy-spinning.
const READ_BLOCK_MS: usize = 5_000;

/// Initial reconnect backoff after a Redis error.
const BACKOFF_INITIAL: Duration = Duration::from_millis(500);

/// Maximum reconnect backoff ceiling.
const BACKOFF_MAX: Duration = Duration::from_secs(30);

/// Reloads the bounded Postgres snapshot and swaps it into the live retriever.
///
/// Implemented in `lib.rs` over the concrete orchestrator + PG/embedding
/// dependencies; the subscriber depends only on this seam so it stays free of
/// generic embedding/PG plumbing and is unit-testable with a fake.
#[async_trait]
pub(crate) trait GraphReloader: Send + Sync {
    /// Reloads the newest durable snapshot and atomically swaps it in.
    ///
    /// Returns the applied graph version on success. Must be idempotent: invoked
    /// for an already-applied version, the underlying `swap_graph` is a no-op and
    /// this still returns `Ok` so the triggering event can be ACKed.
    async fn reload_and_swap(&self) -> Result<i64, String>;
}

/// The outcome of processing one batch of stream messages.
///
/// Carries enough information to drive the ACK loop: whether `graph.rebuilt`
/// messages appeared in the batch (`has_rebuilt`) and whether the coalesced
/// reload succeeded (`reload_succeeded`). A `graph.rebuilt` message is skipped
/// in the ACK loop when `has_rebuilt && !reload_succeeded` — it must replay.
struct BatchOutcome {
    /// Whether at least one `graph.rebuilt` event was present in the batch.
    /// Used in tests to verify coalescing decisions; `process_batch` derives the
    /// same information per-message from `is_rebuilt` so this field is not
    /// re-read there.
    #[allow(dead_code)]
    has_rebuilt: bool,
    /// Whether the coalesced `reload_and_swap` call succeeded (or was not needed).
    reload_succeeded: bool,
}

/// Runs the refresh loop forever (until the task is dropped/aborted).
///
/// Each iteration: read a batch via the consumer group, coalesce any
/// `graph.rebuilt` events into a single reload+swap, then ACK every message in
/// the batch (rebuild and non-rebuild alike, so unrelated events on the shared
/// stream do not pile up as pending for this consumer). On any Redis error it
/// backs off exponentially and retries — it never returns to the caller and
/// never panics.
pub(crate) async fn run_graph_refresh_loop(
    redis_streams: Arc<RedisStreamsAdapter>,
    reloader: Arc<dyn GraphReloader>,
) {
    let mut backoff = BACKOFF_INITIAL;
    info!("graph refresh subscriber started");

    loop {
        match redis_streams
            .read_group(READ_BATCH_SIZE, READ_BLOCK_MS)
            .await
        {
            Ok(messages) => {
                backoff = BACKOFF_INITIAL;
                if messages.is_empty() {
                    continue;
                }
                process_batch(redis_streams.as_ref(), reloader.as_ref(), &messages).await;
            }
            Err(read_error) => {
                error!(
                    %read_error,
                    backoff_ms = backoff.as_millis() as u64,
                    "graph refresh read failed; backing off before reconnect"
                );
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(BACKOFF_MAX);
            }
        }
    }
}

/// Coalesces the batch and ACKs each message only after the reload+swap step it
/// depends on has succeeded.
///
/// Delegates the coalescing decision and the single reload to [`coalesced_reload`]
/// (Redis-free, unit-testable). Uses the returned [`BatchOutcome`] to decide
/// which messages to ACK.
async fn process_batch(
    redis_streams: &RedisStreamsAdapter,
    reloader: &dyn GraphReloader,
    messages: &[StreamMessage],
) {
    let outcome = coalesced_reload(reloader, messages).await;

    for message in messages {
        let is_rebuilt = message.envelope.event_type == GRAPH_REBUILT_EVENT_TYPE;
        if is_rebuilt && !outcome.reload_succeeded {
            // Do not ACK: this event must replay until a reload succeeds.
            continue;
        }
        if let Err(ack_error) = redis_streams.ack(&message.stream_id).await {
            warn!(
                %ack_error,
                stream_id = %message.stream_id,
                event_type = %message.envelope.event_type,
                "failed to ack stream message after processing"
            );
        }
    }
}

/// Performs the coalesced reload for a batch of stream messages.
///
/// If the batch contains at least one `graph.rebuilt` event, a single
/// `reload_and_swap` is performed for the whole batch (N→1 coalescing). If no
/// `graph.rebuilt` events are present, `reload_succeeded` is `true` (no reload
/// is needed, so non-rebuilt messages are always ackable).
///
/// A failed reload sets `reload_succeeded = false` so the ACK loop withholds
/// ACKs for `graph.rebuilt` messages, causing them to replay on the next pending
/// read.
async fn coalesced_reload(
    reloader: &dyn GraphReloader,
    messages: &[StreamMessage],
) -> BatchOutcome {
    let has_rebuilt = messages
        .iter()
        .any(|message| message.envelope.event_type == GRAPH_REBUILT_EVENT_TYPE);

    if !has_rebuilt {
        return BatchOutcome {
            has_rebuilt: false,
            reload_succeeded: true,
        };
    }

    let rebuilt_count = messages
        .iter()
        .filter(|m| m.envelope.event_type == GRAPH_REBUILT_EVENT_TYPE)
        .count();

    match reloader.reload_and_swap().await {
        Ok(applied_version) => {
            info!(
                coalesced_events = rebuilt_count,
                applied_version, "graph refresh applied after graph.rebuilt"
            );
            BatchOutcome {
                has_rebuilt: true,
                reload_succeeded: true,
            }
        }
        Err(reload_error) => {
            warn!(
                %reload_error,
                coalesced_events = rebuilt_count,
                "graph refresh reload failed; leaving graph.rebuilt unacked for replay"
            );
            BatchOutcome {
                has_rebuilt: true,
                reload_succeeded: false,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use chrono::Utc;
    use infrastructure::EventEnvelope;
    use uuid::Uuid;

    use super::*;

    fn rebuilt_message(stream_id: &str, version: i64) -> StreamMessage {
        StreamMessage {
            stream_id: stream_id.to_owned(),
            envelope: EventEnvelope {
                event_id: Uuid::now_v7(),
                event_type: GRAPH_REBUILT_EVENT_TYPE.to_owned(),
                correlation_id: Uuid::now_v7(),
                idempotency_key: format!("graph.rebuilt:{version}"),
                schema_version: 1,
                timestamp: Utc::now(),
                payload: serde_json::json!({ "graph_version": version }),
            },
        }
    }

    fn other_message(stream_id: &str) -> StreamMessage {
        StreamMessage {
            stream_id: stream_id.to_owned(),
            envelope: EventEnvelope {
                event_id: Uuid::now_v7(),
                event_type: "skill.approved".to_owned(),
                correlation_id: Uuid::now_v7(),
                idempotency_key: format!("skill.approved:{stream_id}"),
                schema_version: 1,
                timestamp: Utc::now(),
                payload: serde_json::json!({}),
            },
        }
    }

    struct CountingReloader {
        calls: AtomicUsize,
        succeed: bool,
    }

    #[async_trait]
    impl GraphReloader for CountingReloader {
        async fn reload_and_swap(&self) -> Result<i64, String> {
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            if self.succeed {
                Ok(call as i64 + 1)
            } else {
                Err("simulated reload failure".to_owned())
            }
        }
    }

    /// A burst of `graph.rebuilt` events coalesces into exactly one reload (true N→1).
    /// Drives `coalesced_reload` directly — no Redis adapter needed.
    #[tokio::test]
    async fn batch_of_rebuilt_events_triggers_single_coalesced_reload() {
        let reloader = CountingReloader {
            calls: AtomicUsize::new(0),
            succeed: true,
        };
        let batch = vec![
            rebuilt_message("1-0", 10),
            rebuilt_message("2-0", 11),
            rebuilt_message("3-0", 12),
        ];

        let outcome = coalesced_reload(&reloader, &batch).await;

        assert!(
            outcome.reload_succeeded,
            "a succeeding reloader must yield reload_succeeded=true"
        );
        assert!(
            outcome.has_rebuilt,
            "batch with rebuilt events must have has_rebuilt=true"
        );
        assert_eq!(
            reloader.calls.load(Ordering::SeqCst),
            1,
            "a coalesced batch of 3 graph.rebuilt events must reload exactly once"
        );
    }

    /// A failed reload must NOT mark the batch as succeeded, so `graph.rebuilt`
    /// messages are withheld from ACK and will replay.
    #[tokio::test]
    async fn failing_reload_marks_batch_outcome_as_not_succeeded() {
        let reloader = CountingReloader {
            calls: AtomicUsize::new(0),
            succeed: false,
        };
        let batch = vec![rebuilt_message("1-0", 10), rebuilt_message("2-0", 11)];

        let outcome = coalesced_reload(&reloader, &batch).await;

        assert!(
            !outcome.reload_succeeded,
            "a failing reloader must yield reload_succeeded=false so events replay"
        );
        assert!(
            outcome.has_rebuilt,
            "batch with rebuilt events must have has_rebuilt=true"
        );
        assert_eq!(
            reloader.calls.load(Ordering::SeqCst),
            1,
            "even a failing reload must be attempted exactly once per batch"
        );
    }

    /// A batch with no `graph.rebuilt` events must never trigger a reload and must
    /// always be considered ackable (non-rebuilt events are always safe to ACK).
    #[tokio::test]
    async fn mixed_batch_non_rebuilt_events_are_always_ackable() {
        let reloader = CountingReloader {
            calls: AtomicUsize::new(0),
            succeed: true,
        };
        let batch = vec![
            other_message("1-0"),
            other_message("2-0"),
            other_message("3-0"),
        ];

        let outcome = coalesced_reload(&reloader, &batch).await;

        assert!(
            outcome.reload_succeeded,
            "a batch with no rebuilt events should have reload_succeeded=true (no reload needed)"
        );
        assert!(
            !outcome.has_rebuilt,
            "a batch with no rebuilt events must have has_rebuilt=false"
        );
        assert_eq!(
            reloader.calls.load(Ordering::SeqCst),
            0,
            "a batch with no graph.rebuilt events must not trigger any reload"
        );
    }
}
