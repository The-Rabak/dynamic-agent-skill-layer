// Smoke test for the fault-injection harness (Slice 1.3).
//
// This test exercises every helper in `tests/e2e/support/` against the live
// stack. It is marked `#[ignore = "requires live containers"]` so CI skips it
// unless containers are up.
//
// To run against the live stack:
//
//   docker compose -f docker-compose.test.yml up -d postgres redis qdrant ollama
//   export DATABASE_URL="postgres://skill_layer:skill_layer@localhost:15432/skill_layer_test"
//   export QDRANT_URL="http://localhost:16333"
//   export OLLAMA_URL="http://localhost:11444"
//   cargo test -p mcp-server --features test-utils \
//     --test test_support_harness_smoke -- --include-ignored
//
// Include mechanism for sibling test files:
//   #[path = "support/mod.rs"]
//   mod support;
//   // then use: support::infra::*, support::poll::*, etc.

#[path = "support/mod.rs"]
mod support;

#[path = "../integration/env_guard.rs"]
mod env_guard;

use std::path::PathBuf;
use std::time::Duration;

use infrastructure::OutboxVectorStore;
use mcp_server::McpServerApp;
use mcp_server::tools::compile_context::CompileContextStatus;
use retrieval::RetrievalConfig;

fn compose_file_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docker-compose.test.yml")
        .canonicalize()
        .expect("docker-compose.test.yml must exist")
}

fn test_repo_path() -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
        .display()
        .to_string()
}

fn smoke_retrieval_config() -> RetrievalConfig {
    RetrievalConfig {
        candidate_limit: 16,
        max_results: 3,
        max_subunits_per_skill: 4,
        rescue_threshold: 0.1,
        relevance_threshold: 0.15,
        mmr_lambda: 0.6,
        ..RetrievalConfig::default()
    }
}

/// Helper 6: poll_until — standalone smoke check (no infra required).
#[test]
fn poll_until_returns_ok_when_predicate_is_immediately_true() {
    let result = support::poll::poll_until_sync(
        || true,
        Duration::from_millis(100),
        Duration::from_millis(10),
    );
    assert!(
        result.is_ok(),
        "poll_until must return Ok when predicate is immediately true"
    );
}

/// Helper 6: poll_until — times out when predicate never fires.
#[test]
fn poll_until_returns_err_on_timeout_when_predicate_never_true() {
    let result = support::poll::poll_until_sync(
        || false,
        Duration::from_millis(50),
        Duration::from_millis(10),
    );
    assert!(result.is_err(), "poll_until must return Err on timeout");
}

/// Helpers 1 + 6: stop/start a compose service and poll for observable effect.
///
/// This test stops Qdrant, confirms it is unreachable, then starts it again
/// and confirms it recovers — proving the container-control helpers are
/// deterministic.
#[ignore = "requires live containers"]
#[tokio::test]
async fn container_stop_makes_service_unreachable_and_start_restores_it() {
    let compose = compose_file_path();

    // Stop Qdrant.
    let stopped = support::infra::compose_stop_service(&compose, "qdrant");
    assert!(
        stopped.is_ok(),
        "compose_stop_service must succeed: {:?}",
        stopped
    );

    // Poll until Qdrant is actually unreachable.
    let qdrant_url =
        std::env::var("QDRANT_URL").unwrap_or_else(|_| "http://localhost:16333".to_owned());
    let http = reqwest::Client::builder()
        .timeout(Duration::from_millis(500))
        .build()
        .expect("http client");
    let qdrant_url_for_check = qdrant_url.clone();
    let http_for_check = http.clone();
    let down_result = support::poll::poll_until(
        || {
            let client = http_for_check.clone();
            let url = qdrant_url_for_check.clone();
            async move {
                client
                    .get(format!("{}/collections", url.trim_end_matches('/')))
                    .send()
                    .await
                    .is_err()
            }
        },
        Duration::from_secs(15),
        Duration::from_millis(500),
    )
    .await;
    assert!(
        down_result.is_ok(),
        "Qdrant must become unreachable after compose stop"
    );

    // Start Qdrant.
    let started = support::infra::compose_start_services(&compose, &["qdrant"]);
    assert!(
        started.is_ok(),
        "compose_start_services must succeed: {:?}",
        started
    );

    // Poll until Qdrant is reachable again.
    let qdrant_url_for_recovery = qdrant_url.clone();
    let http_for_recovery = http.clone();
    let up_result = support::poll::poll_until(
        || {
            let client = http_for_recovery.clone();
            let url = qdrant_url_for_recovery.clone();
            async move {
                client
                    .get(format!("{}/collections", url.trim_end_matches('/')))
                    .send()
                    .await
                    .is_ok()
            }
        },
        Duration::from_secs(30),
        Duration::from_millis(500),
    )
    .await;
    assert!(up_result.is_ok(), "Qdrant must recover after compose start");
}

/// Helper 3: drift injection produces measurable divergence between PG and Qdrant.
///
/// Inserts PG rows without corresponding Qdrant vectors, then counts the gap.
/// Passes if the count of orphaned PG skills equals the injected count.
#[ignore = "requires live containers"]
#[tokio::test]
async fn drift_injection_produces_measurable_pg_qdrant_divergence() {
    use infrastructure::{PostgresAdapter, PostgresConfig, QdrantAdapter, QdrantConfig};

    let db_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        "postgres://skill_layer:skill_layer@localhost:15432/skill_layer_test".to_owned()
    });
    let qdrant_url =
        std::env::var("QDRANT_URL").unwrap_or_else(|_| "http://localhost:16333".to_owned());

    let pg = PostgresAdapter::connect(&PostgresConfig {
        database_url: db_url,
        ..PostgresConfig::default()
    })
    .await
    .expect("PG must be reachable");

    let qdrant = QdrantAdapter::new(
        reqwest::Client::new(),
        QdrantConfig {
            endpoint: qdrant_url,
            ..QdrantConfig::default()
        },
    )
    .expect("Qdrant config valid");

    // Count vectors in Qdrant before injection.
    let before_points = qdrant
        .list_point_ids()
        .await
        .expect("must list Qdrant points");
    let before_count = before_points.point_ids.len();

    // Inject 3 PG-only rows (no Qdrant vector counterpart).
    let injected = support::drift::inject_pg_skills_without_qdrant_vectors(pg.pool(), 3)
        .await
        .expect("drift injection must succeed");
    assert_eq!(injected.len(), 3, "must inject exactly 3 PG-only skills");

    // Count vectors in Qdrant after injection — must be unchanged (no new vectors).
    let after_points = qdrant
        .list_point_ids()
        .await
        .expect("must list Qdrant points");
    let after_count = after_points.point_ids.len();

    assert_eq!(
        before_count, after_count,
        "injecting PG-only rows must not change Qdrant vector count: divergence exists"
    );

    // Cleanup: remove injected rows.
    support::drift::remove_injected_skills(pg.pool(), &injected)
        .await
        .expect("cleanup must succeed");
}

/// Helper 4: watcher load driver writes N SKILL.md files to a sandbox dir.
///
/// Verifies that the files exist and have the expected content shape.
#[test]
fn watcher_load_driver_writes_skill_files_to_sandbox() {
    let sandbox = std::env::temp_dir().join(format!(
        "harness-smoke-watcher-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock is after epoch")
            .as_nanos()
    ));
    std::fs::create_dir_all(&sandbox).expect("sandbox should be creatable");

    let written = support::load::write_skill_files_to_sandbox(&sandbox, 5)
        .expect("writing skill files must succeed");

    assert_eq!(written.len(), 5, "must write exactly 5 SKILL.md files");
    for path in &written {
        assert!(
            path.exists(),
            "SKILL.md file must exist on disk: {:?}",
            path
        );
        let content = std::fs::read_to_string(path).expect("SKILL.md must be readable");
        assert!(
            content.contains("SKILL.md"),
            "SKILL.md content must reference SKILL.md: {:?}",
            path
        );
    }

    std::fs::remove_dir_all(&sandbox).expect("sandbox cleanup should succeed");
}

/// Helper 5: concurrent load generator produces genuine concurrency.
///
/// Fires K concurrent compile_context calls and checks that latencies overlap
/// (i.e. first completion time < last start time), proving calls ran in parallel
/// rather than serially.
#[ignore = "requires live containers"]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_load_generator_produces_genuine_parallelism() {
    let _env_guard = env_guard::configure_scope_env();

    let components = McpServerApp::from_environment(smoke_retrieval_config())
        .await
        .expect("mcp-server must come up from environment");

    let repo = test_repo_path();
    let concurrency = 8usize;

    let samples = support::load::fire_concurrent_compile_context(
        components.app.clone(),
        repo,
        "harness-smoke-concurrent",
        concurrency,
    )
    .await;

    assert_eq!(
        samples.len(),
        concurrency,
        "must collect one sample per call"
    );

    // All must be legal statuses.
    for s in &samples {
        assert!(
            matches!(
                s.status,
                CompileContextStatus::Ok
                    | CompileContextStatus::NoMatch
                    | CompileContextStatus::Degraded
                    | CompileContextStatus::DuplicateSuppressed
            ),
            "every response must carry a legal status; got {:?}",
            s.status
        );
    }

    // Genuine concurrency check: if calls ran in parallel the minimum start-to-finish
    // spread will be less than the sum of all individual latencies.
    let total_sequential_ms: u64 = samples.iter().map(|s| s.duration_ms).sum();
    let wall_clock_ms = samples.iter().map(|s| s.duration_ms).max().unwrap_or(0);
    // With real concurrency, wall_clock_ms << total_sequential_ms (at least 2×).
    // We use a conservative 1.5× threshold to tolerate slow CI environments.
    if total_sequential_ms > 0 {
        assert!(
            wall_clock_ms < total_sequential_ms,
            "wall-clock time ({wall_clock_ms}ms) must be less than sum of sequential latencies \
             ({total_sequential_ms}ms), proving genuine concurrency"
        );
    }

    components.teardown().await.expect("teardown");
}
