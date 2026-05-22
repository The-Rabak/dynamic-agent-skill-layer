use std::{future::Future, sync::Arc, time::Duration};

use tokio::{sync::Mutex, time::sleep};

#[derive(Debug, Clone)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub base_delay: Duration,
    pub max_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_delay: Duration::from_millis(50),
            max_delay: Duration::from_secs(2),
        }
    }
}

pub async fn retry_with_backoff<T, E, F, Fut>(
    policy: &RetryPolicy,
    mut operation: F,
) -> Result<T, E>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, E>>,
{
    let mut attempt = 1;
    loop {
        match operation().await {
            Ok(value) => return Ok(value),
            Err(error) if attempt >= policy.max_attempts => return Err(error),
            Err(_) => {
                let jitter = Duration::from_millis((attempt * 7) as u64);
                let mut wait = policy.base_delay.saturating_mul(attempt);
                wait = wait.saturating_add(jitter).min(policy.max_delay);
                sleep(wait).await;
                attempt += 1;
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}

#[derive(Debug)]
struct CircuitStateData {
    state: CircuitState,
    consecutive_failures: u32,
    opened_at: Option<std::time::Instant>,
}

#[derive(Debug, Clone)]
pub struct CircuitBreaker {
    failure_threshold: u32,
    open_for: Duration,
    state: Arc<Mutex<CircuitStateData>>,
}

impl CircuitBreaker {
    pub fn new(failure_threshold: u32, open_for: Duration) -> Self {
        Self {
            failure_threshold: failure_threshold.max(1),
            open_for,
            state: Arc::new(Mutex::new(CircuitStateData {
                state: CircuitState::Closed,
                consecutive_failures: 0,
                opened_at: None,
            })),
        }
    }

    pub async fn allow_request(&self) -> bool {
        let mut state = self.state.lock().await;

        match state.state {
            CircuitState::Closed => true,
            CircuitState::HalfOpen => true,
            CircuitState::Open => {
                let ready_for_half_open = state
                    .opened_at
                    .map(|opened_at| opened_at.elapsed() >= self.open_for)
                    .unwrap_or(false);

                if ready_for_half_open {
                    state.state = CircuitState::HalfOpen;
                    true
                } else {
                    false
                }
            }
        }
    }

    pub async fn record_success(&self) {
        let mut state = self.state.lock().await;
        state.state = CircuitState::Closed;
        state.consecutive_failures = 0;
        state.opened_at = None;
    }

    pub async fn record_failure(&self) {
        let mut state = self.state.lock().await;
        state.consecutive_failures += 1;

        if state.consecutive_failures >= self.failure_threshold {
            state.state = CircuitState::Open;
            state.opened_at = Some(std::time::Instant::now());
        }
    }

    pub async fn state(&self) -> CircuitState {
        self.state.lock().await.state
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    };

    use super::*;

    #[tokio::test]
    async fn retry_eventually_succeeds_within_attempt_limit() {
        let attempts = Arc::new(AtomicU32::new(0));
        let attempts_clone = Arc::clone(&attempts);
        let policy = RetryPolicy {
            max_attempts: 3,
            base_delay: Duration::from_millis(1),
            max_delay: Duration::from_millis(5),
        };

        let result = retry_with_backoff(&policy, move || {
            let attempts = Arc::clone(&attempts_clone);
            async move {
                let current = attempts.fetch_add(1, Ordering::SeqCst);
                if current < 1 {
                    Err("temporary")
                } else {
                    Ok("ok")
                }
            }
        })
        .await;

        assert_eq!(result.expect("operation should eventually pass"), "ok");
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn circuit_breaker_opens_after_threshold() {
        let breaker = CircuitBreaker::new(2, Duration::from_secs(5));

        breaker.record_failure().await;
        breaker.record_failure().await;

        assert_eq!(breaker.state().await, CircuitState::Open);
        assert!(!breaker.allow_request().await);
    }
}
