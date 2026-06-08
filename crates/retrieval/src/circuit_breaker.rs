use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

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
    opened_at: Option<Instant>,
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
        let mut state = self.state.lock().expect("circuit breaker mutex poisoned");

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
        let mut state = self.state.lock().expect("circuit breaker mutex poisoned");
        state.state = CircuitState::Closed;
        state.consecutive_failures = 0;
        state.opened_at = None;
    }

    pub async fn record_failure(&self) {
        let mut state = self.state.lock().expect("circuit breaker mutex poisoned");
        state.consecutive_failures += 1;

        if state.consecutive_failures >= self.failure_threshold {
            state.state = CircuitState::Open;
            state.opened_at = Some(Instant::now());
        }
    }

    pub async fn state(&self) -> CircuitState {
        self.state
            .lock()
            .expect("circuit breaker mutex poisoned")
            .state
    }
}
