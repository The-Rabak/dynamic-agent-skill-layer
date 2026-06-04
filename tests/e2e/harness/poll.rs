/// Bounded readiness and convergence polling helpers for the real-infra E2E harness.
///
/// These replace fixed `sleep` calls.  Every poller returns `Ok(())` when the
/// condition fires within the timeout window, or `Err(String)` with a
/// diagnostic message when the deadline passes.
///
/// # Reuse
/// `poll_until` from `tests/e2e/support/poll.rs` is the primitive — this module
/// builds domain-specific waiters on top of it rather than reimplementing the
/// loop.
use std::time::Duration;

use super::app::{CompileContextArgs, McpClient};
use super::observe::PgObserver;

/// Polls `predicate` asynchronously every `interval` until it returns `true`
/// or `timeout` elapses.
///
/// Returns `Ok(())` if the predicate fires within the window.
/// Returns `Err("timed out after <n>ms waiting for predicate")` when the
/// deadline passes without the predicate returning `true`.
pub async fn poll_until<F, Fut>(
    predicate: F,
    timeout: Duration,
    interval: Duration,
) -> Result<(), String>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if predicate().await {
            return Ok(());
        }
        if std::time::Instant::now() >= deadline {
            return Err(format!(
                "timed out after {}ms waiting for predicate",
                timeout.as_millis()
            ));
        }
        tokio::time::sleep(interval).await;
    }
}

/// Waits for the graph to rebuild such that both:
///   1. `graph_state.graph_version` in Postgres advances past `prev_version`, AND
///   2. A `compile_context` call over HTTP at `:3001` reports a `graph_version > prev_version`.
///
/// This is the primary convergence check for the golden-path test.  It proves
/// the full pipeline loop: file → graph-builder rebuild → mcp-server snapshot swap.
///
/// # Known bug #156
/// graph-builder currently bumps `graph_version` in PG but ERRORS on the outbox
/// idempotency conflict BEFORE publishing `graph.rebuilt` to Redis, so the
/// mcp-server snapshot NEVER advances.  This function will therefore time out at
/// `timeout` and return an explicit `Err` message that names the bug.
///
/// The timeout prevents the test from hanging forever; the error message is the
/// regression guard for the #156 fix.
pub async fn wait_for_rebuild(prev_version: i64, timeout: Duration) -> Result<(), String> {
    let pg = PgObserver::connect().await;
    let client = McpClient::new();

    let probe_session = format!(
        "wait-for-rebuild-probe-{}",
        chrono::Utc::now().timestamp_millis()
    );

    let deadline = std::time::Instant::now() + timeout;
    let interval = Duration::from_millis(500);

    loop {
        // Check 1: PG graph_version has advanced.
        let pg_advanced = pg
            .graph_version()
            .await
            .map(|v| v > prev_version)
            .unwrap_or(false);

        // Check 2: mcp-server's served graph_version has advanced.
        // Use a unique session_id each probe to avoid duplicate-suppression.
        let served_version_probe = client
            .compile_context(CompileContextArgs {
                prompt: "rebuild convergence probe".to_owned(),
                session_id: format!(
                    "{probe_session}-{}",
                    std::time::Instant::now().elapsed().as_millis()
                ),
                repo_path: "/tmp".to_owned(),
                trigger: None,
            })
            .await
            .map(|r| r.graph_version)
            .unwrap_or(prev_version);

        if pg_advanced && served_version_probe > prev_version {
            return Ok(());
        }

        if std::time::Instant::now() >= deadline {
            let pg_ver = pg.graph_version().await.unwrap_or(-1);
            return Err(format!(
                "snapshot did not advance from v{prev_version} within {}s — see #156\n\
                 PG graph_version={pg_ver}, served graph_version={served_version_probe}\n\
                 Root cause: graph-builder bumps graph_state then errors on outbox \
                 idempotency conflict before publishing graph.rebuilt, so the \
                 mcp-server refresh subscriber never fires.",
                timeout.as_secs()
            ));
        }

        tokio::time::sleep(interval).await;
    }
}

/// Waits for `GET /health` on the mcp-server to return `{"healthy":true}`.
///
/// Returns `Ok(())` when healthy within `timeout`, `Err` otherwise.
pub async fn wait_for_health(timeout: Duration) -> Result<(), String> {
    let client = McpClient::new();
    let deadline = std::time::Instant::now() + timeout;
    let interval = Duration::from_millis(500);

    loop {
        if let Ok((code, body)) = client.health().await {
            if code == 200 && body.get("healthy") == Some(&serde_json::Value::Bool(true)) {
                return Ok(());
            }
        }
        if std::time::Instant::now() >= deadline {
            return Err(format!(
                "mcp-server did not become healthy within {}s",
                timeout.as_secs()
            ));
        }
        tokio::time::sleep(interval).await;
    }
}
