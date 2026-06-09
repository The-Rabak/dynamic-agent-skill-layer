//! Live proof that a per-run test sandbox (#164's `NamespaceGuard`) is reclaimed
//! even when the test never reaches `cleanup().await` — i.e. when it panics.
//!
//! `NamespaceGuard::cleanup` is async and only runs on the happy path. A panicking
//! test would otherwise leak its `test_ns_<runid>` PG schema, `skills_ns_<runid>`
//! Qdrant collection, and Redis stream/keys forever (we found real leaked sandboxes
//! on the live stack — the motivation for this fix). The guard's `Drop` impl now
//! runs the same teardown synchronously on a dedicated thread, so the sandbox is
//! reclaimed whether the test calls `cleanup()`, forgets to, or panics.
//!
//! Two proofs:
//! 1. `drop_reclaims_sandbox_when_cleanup_is_skipped` — drops the guard WITHOUT
//!    calling `cleanup()`. Rust runs `Drop` identically on an explicit `drop` and on
//!    an unwind, so this faithfully reproduces the post-panic state, deterministically.
//! 2. `real_panic_during_test_still_reclaims_sandbox` — drives an ACTUAL panic
//!    through an unwinding worker thread holding a live sandbox, then asserts the
//!    sandbox is gone. Proves the `Drop` fallback fires during real unwinding.
//!
//! Run (requires the live test stack — PG :15432, Qdrant :16333, Redis :16379):
//! ```bash
//! cargo test -p mcp-server --features test-utils \
//!   --test test_namespace_cleanup_on_panic -- --ignored --nocapture
//! ```
//!
//! No in-process fakes: the whole point is to exercise real `DROP SCHEMA` /
//! Qdrant `DELETE` / Redis `DEL` against the live containers.

use std::env;

#[path = "../integration/env_guard.rs"]
mod env_guard;

// ---- live-infra existence probes (real connections, no stubs) ----

async fn pg_schema_exists(base_db_url: &str, schema: &str) -> bool {
    let pool = sqlx::PgPool::connect(base_db_url)
        .await
        .expect("admin pool connects");
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_namespace WHERE nspname = $1)")
            .bind(schema)
            .fetch_one(&pool)
            .await
            .expect("schema existence query");
    pool.close().await;
    exists
}

async fn qdrant_collection_exists(qdrant_url: &str, collection: &str) -> bool {
    let url = format!(
        "{}/collections/{collection}",
        qdrant_url.trim_end_matches('/')
    );
    let resp = reqwest::Client::new()
        .get(url)
        .send()
        .await
        .expect("qdrant reachable");
    resp.status().is_success()
}

async fn create_qdrant_collection(qdrant_url: &str, collection: &str) {
    let url = format!(
        "{}/collections/{collection}",
        qdrant_url.trim_end_matches('/')
    );
    let resp = reqwest::Client::new()
        .put(url)
        .json(&serde_json::json!({
            "vectors": { "size": 768, "distance": "Cosine" }
        }))
        .send()
        .await
        .expect("create sandbox collection");
    assert!(
        resp.status().is_success(),
        "sandbox collection PUT must succeed, got {}",
        resp.status()
    );
}

async fn redis_key_exists(redis_url: &str, key: &str) -> bool {
    let client = redis::Client::open(redis_url.to_owned()).expect("redis client");
    let mut conn = client
        .get_multiplexed_async_connection()
        .await
        .expect("redis connection");
    let exists: bool = redis::cmd("EXISTS")
        .arg(key)
        .query_async(&mut conn)
        .await
        .expect("EXISTS query");
    exists
}

async fn redis_set(redis_url: &str, key: &str, value: &str) {
    let client = redis::Client::open(redis_url.to_owned()).expect("redis client");
    let mut conn = client
        .get_multiplexed_async_connection()
        .await
        .expect("redis connection");
    let _: () = redis::cmd("SET")
        .arg(key)
        .arg(value)
        .query_async(&mut conn)
        .await
        .expect("SET");
}

async fn redis_xadd(redis_url: &str, stream_key: &str) {
    let client = redis::Client::open(redis_url.to_owned()).expect("redis client");
    let mut conn = client
        .get_multiplexed_async_connection()
        .await
        .expect("redis connection");
    let _: String = redis::cmd("XADD")
        .arg(stream_key)
        .arg("*")
        .arg("probe")
        .arg("1")
        .query_async(&mut conn)
        .await
        .expect("XADD");
}

/// Proof 1: dropping the guard without calling `cleanup()` reclaims the full
/// sandbox. This is exactly the state a panic leaves (`cleanup()` never reached),
/// and `Drop` runs the same on explicit-drop and unwind.
#[tokio::test]
#[ignore = "requires live PG/Qdrant/Redis (test stack)"]
async fn drop_reclaims_sandbox_when_cleanup_is_skipped() {
    // Capture base URLs BEFORE the guard rewrites DATABASE_URL to the sandbox.
    let base_db = env::var("DATABASE_URL").expect("DATABASE_URL must be set for live tests");
    let qdrant = env::var("QDRANT_URL").expect("QDRANT_URL must be set for live tests");
    let redis_url = env::var("REDIS_URL").expect("REDIS_URL must be set for live tests");

    let guard = env_guard::isolated_namespace().await;
    let schema = guard.schema().to_owned();
    let collection = env::var("QDRANT_COLLECTION").expect("guard set QDRANT_COLLECTION");
    let stream_key = env::var("REDIS_STREAM_KEY").expect("guard set REDIS_STREAM_KEY");
    let prefix = env::var("REDIS_KEY_PREFIX").expect("guard set REDIS_KEY_PREFIX");
    let probe_key = format!("{prefix}suppression:probe::x");

    // Populate the sandbox the way a real test would before it falls over.
    create_qdrant_collection(&qdrant, &collection).await;
    redis_set(&redis_url, &probe_key, "1").await;
    redis_xadd(&redis_url, &stream_key).await;

    // Sanity: every sandbox resource exists while the guard is live.
    assert!(
        pg_schema_exists(&base_db, &schema).await,
        "sandbox schema must exist while the guard is live"
    );
    assert!(
        qdrant_collection_exists(&qdrant, &collection).await,
        "sandbox collection must exist while the guard is live"
    );
    assert!(
        redis_key_exists(&redis_url, &probe_key).await,
        "sandbox suppression key must exist while the guard is live"
    );
    assert!(
        redis_key_exists(&redis_url, &stream_key).await,
        "sandbox stream must exist while the guard is live"
    );

    // Drop WITHOUT cleanup() — precisely what a panicking test leaves behind.
    // The Drop fallback runs the teardown synchronously (spawns a thread + joins),
    // so by the time `drop` returns, reclamation is complete.
    drop(guard);

    assert!(
        !pg_schema_exists(&base_db, &schema).await,
        "Drop fallback must reclaim the sandbox PG schema {schema}"
    );
    assert!(
        !qdrant_collection_exists(&qdrant, &collection).await,
        "Drop fallback must reclaim the sandbox Qdrant collection {collection}"
    );
    assert!(
        !redis_key_exists(&redis_url, &probe_key).await,
        "Drop fallback must reclaim the sandbox suppression key"
    );
    assert!(
        !redis_key_exists(&redis_url, &stream_key).await,
        "Drop fallback must reclaim the sandbox Redis stream"
    );
}

/// Proof 2: an ACTUAL panic while a sandbox is held still reclaims it. The sandbox
/// is built and populated inside a worker thread that then panics; the guard is
/// dropped during the real unwind, and the main thread asserts the sandbox is gone.
#[tokio::test]
#[ignore = "requires live PG/Qdrant/Redis (test stack)"]
async fn real_panic_during_test_still_reclaims_sandbox() {
    let base_db = env::var("DATABASE_URL").expect("DATABASE_URL must be set for live tests");
    let qdrant = env::var("QDRANT_URL").expect("QDRANT_URL must be set for live tests");
    let redis_url = env::var("REDIS_URL").expect("REDIS_URL must be set for live tests");

    // The worker reports its sandbox names through this channel BEFORE it panics,
    // so the main thread knows what to check for after the unwind.
    let (tx, rx) = std::sync::mpsc::channel::<(String, String, String)>();
    let qdrant_for_worker = qdrant.clone();
    let redis_for_worker = redis_url.clone();

    let worker = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("worker runtime builds");
        rt.block_on(async move {
            let guard = env_guard::isolated_namespace().await;
            let schema = guard.schema().to_owned();
            let collection = env::var("QDRANT_COLLECTION").expect("guard set QDRANT_COLLECTION");
            let prefix = env::var("REDIS_KEY_PREFIX").expect("guard set REDIS_KEY_PREFIX");
            let probe_key = format!("{prefix}suppression:probe::x");

            create_qdrant_collection(&qdrant_for_worker, &collection).await;
            redis_set(&redis_for_worker, &probe_key, "1").await;

            // Hand the names to the main thread, THEN fall over with the guard live.
            tx.send((schema, collection, probe_key))
                .expect("send sandbox names before panic");
            panic!("simulated test failure while holding a live sandbox");
            // `guard` drops here during unwinding → Drop fallback reclaims the sandbox.
        });
    });

    let (schema, collection, probe_key) = rx.recv().expect("worker reports names before panicking");
    let joined = worker.join();
    assert!(
        joined.is_err(),
        "the worker thread must have actually panicked (else the proof is vacuous)"
    );

    assert!(
        !pg_schema_exists(&base_db, &schema).await,
        "Drop during real unwind must reclaim the sandbox PG schema {schema}"
    );
    assert!(
        !qdrant_collection_exists(&qdrant, &collection).await,
        "Drop during real unwind must reclaim the sandbox Qdrant collection {collection}"
    );
    assert!(
        !redis_key_exists(&redis_url, &probe_key).await,
        "Drop during real unwind must reclaim the sandbox suppression key"
    );
}
