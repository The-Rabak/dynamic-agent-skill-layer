/// Bounded readiness and convergence polling helpers.
///
/// These replace fixed `sleep` calls throughout the E2E suite. Every poller
/// returns `Ok(())` when the predicate fires within the timeout window, or
/// `Err(String)` with a diagnostic message when it expires.
use std::{future::Future, time::Duration};

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
    Fut: Future<Output = bool>,
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

/// Synchronous variant of [`poll_until`] for use in non-async tests.
///
/// Spins in a busy-poll loop checking `predicate` every `interval`. Use only
/// for very short timeouts (< 1 second) to avoid blocking the test thread.
pub fn poll_until_sync<F>(predicate: F, timeout: Duration, interval: Duration) -> Result<(), String>
where
    F: Fn() -> bool,
{
    let deadline = std::time::Instant::now() + timeout;

    loop {
        if predicate() {
            return Ok(());
        }
        if std::time::Instant::now() >= deadline {
            return Err(format!(
                "timed out after {}ms waiting for predicate",
                timeout.as_millis()
            ));
        }
        std::thread::sleep(interval);
    }
}
