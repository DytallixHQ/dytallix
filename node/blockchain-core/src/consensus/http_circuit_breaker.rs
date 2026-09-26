//! Opt-in HTTP availability control. This module does not authenticate oracle results.
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Failure threshold uses basis points: 5,000 means 50%.
#[derive(Debug, Clone, Copy)]
pub struct HttpCircuitBreakerConfig {
    pub failure_threshold_bps: u16,
    pub min_requests: usize,
    pub window_size: usize,
    pub recovery_timeout: Duration,
}

impl HttpCircuitBreakerConfig {
    fn validate(self) -> Result<Self, HttpCircuitError> {
        if !(1..=10_000).contains(&self.failure_threshold_bps)
            || !(1..=65_536).contains(&self.window_size)
            || self.min_requests == 0
            || self.min_requests > self.window_size
            || self.recovery_timeout.is_zero()
        {
            return Err(HttpCircuitError::InvalidConfig);
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpCircuitState {
    Closed,
    Open,
    HalfOpen,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum HttpCircuitError {
    #[error("invalid HTTP circuit-breaker configuration")]
    InvalidConfig,
    #[error("HTTP circuit is open")]
    Open,
    #[error("HTTP circuit recovery probe is already in progress")]
    ProbeInProgress,
    #[error("HTTP circuit state is unavailable")]
    StateUnavailable,
}

/// Counters saturate at u64::MAX and reset only on explicit reset.
/// Window counts exclude cancellations and results from an earlier circuit state.
#[derive(Debug, Clone, Copy)]
pub struct HttpCircuitStatus {
    pub state: HttpCircuitState,
    pub admitted: u64,
    pub rejected: u64,
    pub succeeded: u64,
    pub failed: u64,
    pub cancelled: u64,
    pub window_requests: usize,
    pub window_failures: usize,
}

#[derive(Debug)]
struct State {
    status: HttpCircuitStatus,
    outcomes: VecDeque<bool>,
    opened_at: Option<Instant>,
    // Separate identity tokens prevent stale completions after reset or transition.
    reset: Arc<()>,
    generation: Arc<()>,
}

impl State {
    fn new() -> Self {
        Self {
            status: HttpCircuitStatus {
                state: HttpCircuitState::Closed,
                admitted: 0,
                rejected: 0,
                succeeded: 0,
                failed: 0,
                cancelled: 0,
                window_requests: 0,
                window_failures: 0,
            },
            outcomes: VecDeque::new(),
            opened_at: None,
            reset: Arc::new(()),
            generation: Arc::new(()),
        }
    }

    fn open(&mut self) {
        self.status.state = HttpCircuitState::Open;
        self.opened_at = Some(Instant::now());
        self.generation = Arc::new(());
    }
}

#[derive(Debug)]
pub(crate) struct HttpCircuitBreaker {
    config: HttpCircuitBreakerConfig,
    state: Mutex<State>,
}

impl HttpCircuitBreaker {
    pub(crate) fn new(config: HttpCircuitBreakerConfig) -> Result<Self, HttpCircuitError> {
        Ok(Self {
            config: config.validate()?,
            state: Mutex::new(State::new()),
        })
    }

    pub(crate) fn status(&self) -> Result<HttpCircuitStatus, HttpCircuitError> {
        Ok(self
            .state
            .lock()
            .map_err(|_| HttpCircuitError::StateUnavailable)?
            .status)
    }

    pub(crate) fn reset(&self) -> Result<(), HttpCircuitError> {
        *self
            .state
            .lock()
            .map_err(|_| HttpCircuitError::StateUnavailable)? = State::new();
        Ok(())
    }

    pub(crate) fn acquire(&self) -> Result<RequestPermit<'_>, HttpCircuitError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| HttpCircuitError::StateUnavailable)?;
        if state.status.state == HttpCircuitState::Open
            && state
                .opened_at
                .is_some_and(|at| at.elapsed() >= self.config.recovery_timeout)
        {
            state.status.state = HttpCircuitState::HalfOpen;
            state.generation = Arc::new(());
        } else if state.status.state != HttpCircuitState::Closed {
            state.status.rejected = state.status.rejected.saturating_add(1);
            return Err(if state.status.state == HttpCircuitState::Open {
                HttpCircuitError::Open
            } else {
                HttpCircuitError::ProbeInProgress
            });
        }
        state.status.admitted = state.status.admitted.saturating_add(1);
        Ok(RequestPermit {
            breaker: self,
            reset: state.reset.clone(),
            generation: state.generation.clone(),
            probe: state.status.state == HttpCircuitState::HalfOpen,
            finished: false,
        })
    }
}

/// Dropping a cancelled recovery request reopens the circuit for a full interval.
pub(crate) struct RequestPermit<'a> {
    breaker: &'a HttpCircuitBreaker,
    reset: Arc<()>,
    generation: Arc<()>,
    probe: bool,
    finished: bool,
}

impl RequestPermit<'_> {
    pub(crate) fn is_probe(&self) -> bool {
        self.probe
    }

    pub(crate) fn finish(mut self, success: bool) {
        self.record(Some(success));
        self.finished = true;
    }

    fn record(&self, outcome: Option<bool>) {
        // A poisoned state denies later requests. Drop must not panic.
        let Ok(mut state) = self.breaker.state.lock() else {
            return;
        };
        if !Arc::ptr_eq(&self.reset, &state.reset) {
            return;
        }
        let counter = match outcome {
            Some(true) => &mut state.status.succeeded,
            Some(false) => &mut state.status.failed,
            None => &mut state.status.cancelled,
        };
        *counter = counter.saturating_add(1);
        if !Arc::ptr_eq(&self.generation, &state.generation) {
            return;
        }
        if self.probe {
            if outcome == Some(true) {
                state.status.state = HttpCircuitState::Closed;
                state.opened_at = None;
                state.outcomes.clear();
                state.status.window_requests = 0;
                state.status.window_failures = 0;
                state.generation = Arc::new(());
            } else {
                state.open();
            }
        } else if let Some(success) = outcome {
            if state.outcomes.len() == self.breaker.config.window_size
                && state.outcomes.pop_front() == Some(false)
            {
                state.status.window_failures -= 1;
            }
            state.outcomes.push_back(success);
            state.status.window_requests = state.outcomes.len();
            if !success {
                state.status.window_failures += 1;
            }
            if state.status.window_requests >= self.breaker.config.min_requests
                && (state.status.window_failures as u64) * 10_000
                    >= u64::from(self.breaker.config.failure_threshold_bps)
                        * (state.status.window_requests as u64)
            {
                state.open();
            }
        }
    }
}

impl Drop for RequestPermit<'_> {
    fn drop(&mut self) {
        if !self.finished {
            self.record(None);
        }
    }
}
