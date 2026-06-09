//! Live proof that `RedisStreamsAdapter::read_group` self-heals after the
//! consumer group is destroyed out from under it (the #163 root cause).
//!
//! Mechanism reproduced: a sibling consumer/test destroys the shared stream with
//! `DEL` (which silently drops every consumer group), a publisher then re-creates
//! the stream via `XADD` (without any group), and the surviving consumer's
//! `XREADGROUP` would fail with `NOGROUP` forever. After the fix, `read_group`
//! re-creates the group at id `0` and re-reads the already-published event.
//!
//! Run (requires the live test stack's Redis on :16379):
//! ```bash
//! REDIS_URL="redis://localhost:16379" \
//!   cargo test -p mcp-server --features test-utils --test test_redis_group_self_heal -- --ignored --nocapture
//! ```
//!
//! `#[ignore]` because it needs a real Redis; it is wired into
//! `scripts/run-e2e-tests.sh` alongside the other live data-plane tests. There is
//! no in-process fake — the whole point is to exercise real `XREADGROUP`/`DEL`
//! semantics, which an in-memory stub cannot reproduce.

use std::time::Duration;

use infrastructure::{EventEnvelope, RedisStreamsAdapter, RedisStreamsConfig};

/// A unique namespace per run so this test never touches the live containers'
/// canonical `skill-layer-events` stream/group — exactly the isolation the wider
/// #163 fix mandates for destructive tests.
fn unique_config() -> RedisStreamsConfig {
    let redis_url =
        std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://localhost:16379".to_owned());
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock after epoch")
        .as_nanos();
    RedisStreamsConfig {
        redis_url,
        stream_key: format!("self-heal-test-events-{nonce}"),
        consumer_group: format!("self-heal-test-group-{nonce}"),
        consumer_name: "worker-1".to_owned(),
        ..RedisStreamsConfig::default()
    }
}

fn rebuilt_event(version: i64) -> EventEnvelope {
    EventEnvelope::new(
        "graph.rebuilt",
        format!("graph.rebuilt:{version}"),
        serde_json::json!({ "graph_version": version }),
    )
}

#[tokio::test]
#[ignore = "requires live Redis (test stack :16379)"]
async fn read_group_recovers_after_stream_deletion() {
    let config = unique_config();
    let adapter = RedisStreamsAdapter::new(config).expect("adapter builds from valid config");

    // Baseline: group exists, an event is published and consumed normally.
    adapter
        .ensure_consumer_group()
        .await
        .expect("initial consumer group creation succeeds");
    adapter
        .publish(&rebuilt_event(1))
        .await
        .expect("publish baseline event");

    let first = adapter
        .read_group(16, 1_000)
        .await
        .expect("baseline read succeeds");
    assert_eq!(
        first.len(),
        1,
        "baseline read must deliver the first published event"
    );
    assert_eq!(first[0].envelope.event_type, "graph.rebuilt");
    adapter
        .ack(&first[0].stream_id)
        .await
        .expect("ack baseline event");

    // Contamination: a destructive teardown elsewhere DELs the shared stream,
    // destroying the consumer group. A publisher then re-creates the stream via
    // XADD — with NO group attached. This is precisely what wedged the live
    // mcp-server subscriber for 17h in #163.
    adapter
        .delete_stream()
        .await
        .expect("delete_stream simulates a sibling's destructive teardown");
    adapter
        .publish(&rebuilt_event(2))
        .await
        .expect("publish after deletion recreates the stream without a group");

    // The self-heal: read_group must detect NOGROUP, recreate the group at id 0,
    // and re-read the event published after the deletion. Before the fix this
    // returned a NOGROUP error and the loop never recovered.
    let recovered = tokio::time::timeout(Duration::from_secs(5), adapter.read_group(16, 1_000))
        .await
        .expect("read_group must not hang while self-healing")
        .expect("read_group must self-heal on NOGROUP and return Ok, not error");

    assert_eq!(
        recovered.len(),
        1,
        "after self-heal, the post-deletion event must be re-read (got {} messages)",
        recovered.len()
    );
    assert_eq!(
        recovered[0].envelope.idempotency_key, "graph.rebuilt:2",
        "the recovered event must be the one published after the stream was recreated"
    );

    // Cleanup our own namespace only.
    adapter
        .ack(&recovered[0].stream_id)
        .await
        .expect("ack recovered event");
    adapter
        .delete_stream()
        .await
        .expect("cleanup self-heal test stream");
}
