use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// Circuit breaker states
#[derive(Debug, Clone, PartialEq)]
pub enum CircuitBreakerState {
    /// Circuit is closed - requests flow normally
    Closed,
    /// Circuit is open - requests are blocked
    Open,
    /// Circuit is half-open - testing if service recovered
    HalfOpen,
}

/// Circuit breaker statistics
#[derive(Debug, Clone)]
pub struct CircuitBreakerStats {
    /// Number of successful requests
    pub success_count: u64,
    /// Number of failed requests
    pub failure_count: u64,
    /// Total number of requests
    pub total_requests: u64,
    /// Current failure rate (0.0 to 1.0)
    pub failure_rate: f64,
    /// Time when circuit was last opened
    pub last_opened_time: Option<std::time::Instant>,
    /// Time when circuit was last closed
    pub last_closed_time: Option<std::time::Instant>,
}

impl Default for CircuitBreakerStats {
    fn default() -> Self {
        Self {
            success_count: 0,
            failure_count: 0,
            total_requests: 0,
            failure_rate: 0.0,
            last_opened_time: None,
            last_closed_time: None,
        }
    }
}

impl CircuitBreakerStats {
    /// Record a successful request
    pub fn record_success(&mut self) {
        self.success_count += 1;
        self.total_requests += 1;
        self.update_failure_rate();
    }

    /// Record a failed request
    pub fn record_failure(&mut self) {
        self.failure_count += 1;
        self.total_requests += 1;
        self.update_failure_rate();
    }

    /// Update the failure rate calculation
    fn update_failure_rate(&mut self) {
        if self.total_requests > 0 {
            self.failure_rate = self.failure_count as f64 / self.total_requests as f64;
        } else {
            self.failure_rate = 0.0;
        }
    }

    /// Reset statistics
    pub fn reset(&mut self) {
        self.success_count = 0;
        self.failure_count = 0;
        self.total_requests = 0;
        self.failure_rate = 0.0;
        self.last_opened_time = None;
        self.last_closed_time = None;
    }
}

/// Configuration for connection pooling and keep-alive settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionPoolConfig {
    /// Maximum number of idle connections per host
    pub max_idle_per_host: usize,
    /// Timeout for idle connections in the pool
    pub idle_timeout: Duration,
    /// TCP keep-alive interval
    pub tcp_keepalive: Duration,
    /// Maximum total connections in the pool
    pub max_total_connections: Option<usize>,
    /// Enable HTTP/2 support
    pub http2_prior_knowledge: bool,
}

impl Default for ConnectionPoolConfig {
    fn default() -> Self {
        Self {
            max_idle_per_host: 10,
            idle_timeout: Duration::from_secs(90),
            tcp_keepalive: Duration::from_secs(60),
            max_total_connections: Some(100),
            http2_prior_knowledge: false,
        }
    }
}

impl ConnectionPoolConfig {
    /// Create a new connection pool configuration for high-performance scenarios
    pub fn high_performance() -> Self {
        Self {
            max_idle_per_host: 50,
            idle_timeout: Duration::from_secs(120),
            tcp_keepalive: Duration::from_secs(30),
            max_total_connections: Some(500),
            http2_prior_knowledge: true,
        }
    }

    /// Create a new connection pool configuration for low-resource scenarios
    pub fn low_resource() -> Self {
        Self {
            max_idle_per_host: 2,
            idle_timeout: Duration::from_secs(30),
            tcp_keepalive: Duration::from_secs(120),
            max_total_connections: Some(10),
            http2_prior_knowledge: false,
        }
    }
}

/// Configuration for retry logic with exponential backoff
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retry attempts
    pub max_attempts: u32,
    /// Base delay for exponential backoff
    pub base_delay: Duration,
    /// Maximum delay cap
    pub max_delay: Duration,
    /// Jitter factor (0.0 to 1.0)
    pub jitter_factor: f64,
    /// Multiplier for exponential backoff
    pub backoff_multiplier: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(30),
            jitter_factor: 0.1,
            backoff_multiplier: 2.0,
        }
    }
}

impl RetryConfig {
    /// Create a new retry configuration
    pub fn new(max_attempts: u32, base_delay: Duration, max_delay: Duration) -> Self {
        Self {
            max_attempts,
            base_delay,
            max_delay,
            jitter_factor: 0.1,
            backoff_multiplier: 2.0,
        }
    }

    /// Create aggressive retry configuration (fast and frequent)
    pub fn aggressive() -> Self {
        Self {
            max_attempts: 5,
            base_delay: Duration::from_millis(50),
            max_delay: Duration::from_secs(5),
            jitter_factor: 0.05,
            backoff_multiplier: 1.5,
        }
    }

    /// Create conservative retry configuration (slow and few)
    pub fn conservative() -> Self {
        Self {
            max_attempts: 2,
            base_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(60),
            jitter_factor: 0.2,
            backoff_multiplier: 3.0,
        }
    }

    /// Create optimized retry configuration for high-performance scenarios
    pub fn optimized() -> Self {
        Self {
            max_attempts: 4,
            base_delay: Duration::from_millis(75),
            max_delay: Duration::from_secs(10),
            jitter_factor: 0.1,
            backoff_multiplier: 1.8,
        }
    }
}

/// Circuit breaker configuration and state
#[derive(Debug, Clone)]
pub struct CircuitBreakerContext {
    /// Current state of the circuit breaker
    pub state: CircuitBreakerState,
    /// Statistics for the circuit breaker
    pub stats: CircuitBreakerStats,
    /// Failure threshold (0.0 to 1.0)
    pub failure_threshold: f64,
    /// Recovery time in seconds
    pub recovery_time_seconds: u64,
    /// Minimum requests before circuit can open
    pub min_requests: u64,
}

impl Default for CircuitBreakerContext {
    fn default() -> Self {
        Self::new(0.5, 60)
    }
}

/// The failure threshold must use a finite ratio, not a failure count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("failure threshold must be a finite ratio from 0.0 through 1.0")]
pub struct InvalidCircuitFailureThreshold;

impl CircuitBreakerContext {
    /// Create a context with a failure ratio and a recovery interval in seconds.
    /// At least ten recorded requests are required before the context opens.
    ///
    /// # Panics
    /// Panics if the threshold is not a finite ratio from 0.0 through 1.0.
    /// Use `try_new` when the configuration requires error handling.
    #[track_caller]
    pub fn new(failure_threshold: f64, recovery_time_seconds: u64) -> Self {
        Self::try_new(failure_threshold, recovery_time_seconds)
            .expect("invalid circuit-breaker failure threshold")
    }

    /// Validate constructor inputs without changing the ratio or its units.
    pub fn try_new(
        failure_threshold: f64,
        recovery_time_seconds: u64,
    ) -> Result<Self, InvalidCircuitFailureThreshold> {
        if !failure_threshold.is_finite() || !(0.0..=1.0).contains(&failure_threshold) {
            return Err(InvalidCircuitFailureThreshold);
        }
        Ok(Self {
            state: CircuitBreakerState::Closed,
            stats: CircuitBreakerStats::default(),
            failure_threshold,
            recovery_time_seconds,
            min_requests: 10,
        })
    }

    /// Reset circuit breaker to closed state
    pub fn reset(&mut self) {
        self.state = CircuitBreakerState::Closed;
        self.stats = CircuitBreakerStats::default();
    }

    /// Check if circuit breaker should open
    pub fn should_open(&self) -> bool {
        if self.stats.total_requests < self.min_requests {
            return false;
        }
        self.stats.failure_rate >= self.failure_threshold
    }

    /// Check if circuit breaker should close
    pub fn should_close(&self) -> bool {
        self.state == CircuitBreakerState::HalfOpen
            && self.stats.failure_rate < self.failure_threshold
    }

    /// Check if circuit breaker should move to half-open
    pub fn should_half_open(&self) -> bool {
        self.should_half_open_at(Instant::now())
    }

    fn should_half_open_at(&self, now: Instant) -> bool {
        if self.state != CircuitBreakerState::Open {
            return false;
        }

        if let Some(last_opened) = self.stats.last_opened_time {
            let elapsed = now.saturating_duration_since(last_opened);
            elapsed.as_secs() >= self.recovery_time_seconds
        } else {
            false
        }
    }

    /// Update circuit breaker state
    pub fn update_state(&mut self, new_state: CircuitBreakerState) {
        match new_state {
            CircuitBreakerState::Open => {
                self.stats.last_opened_time = Some(Instant::now());
            }
            CircuitBreakerState::Closed => {
                self.stats.last_closed_time = Some(Instant::now());
            }
            CircuitBreakerState::HalfOpen => {
                // No specific timestamp update needed for half-open
            }
        }

        self.state = new_state;
    }

    pub fn is_closed(&self) -> bool {
        self.state == CircuitBreakerState::Closed
    }
    pub fn is_open(&self) -> bool {
        self.state == CircuitBreakerState::Open
    }
    pub fn should_allow_request(&self) -> bool {
        self.is_closed()
    }
    pub fn record_failure(&mut self) {
        self.stats.record_failure();
        if self.should_open() {
            self.update_state(CircuitBreakerState::Open);
        }
    }
    pub fn record_success(&mut self, _latency_ms: u64) {
        self.stats.record_success();
        if self.state == CircuitBreakerState::HalfOpen && self.should_close() {
            self.update_state(CircuitBreakerState::Closed);
        }
    }
    pub fn failure_rate(&self) -> f64 {
        self.stats.failure_rate
    }
    pub fn stats(&self) -> &CircuitBreakerStats {
        &self.stats
    }
}

#[cfg(test)]
mod circuit_context_tests {
    use super::*;

    #[test]
    fn rejects_invalid_failure_ratios() {
        for threshold in [
            -1.0,
            -f64::EPSILON,
            1.0 + f64::EPSILON,
            3.0,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ] {
            assert!(matches!(
                CircuitBreakerContext::try_new(threshold, 60),
                Err(InvalidCircuitFailureThreshold)
            ));
        }
        for threshold in [0.0, 0.5, 1.0] {
            let circuit = CircuitBreakerContext::try_new(threshold, 60).unwrap();
            assert_eq!(circuit.failure_threshold, threshold);
            assert_eq!(circuit.recovery_time_seconds, 60);
            assert_eq!(circuit.min_requests, 10);
        }
    }

    #[test]
    #[should_panic(expected = "invalid circuit-breaker failure threshold")]
    fn infallible_constructor_rejects_failure_counts() {
        CircuitBreakerContext::new(3.0, 60);
    }

    #[test]
    fn opens_at_or_above_the_failure_ratio() {
        for (failures, expected_open) in [(4, false), (5, true), (6, true)] {
            let mut circuit = CircuitBreakerContext::new(0.5, 60);
            for _ in 0..10 - failures {
                circuit.record_success(1);
            }
            for _ in 0..failures {
                circuit.record_failure();
            }
            assert_eq!(circuit.stats.total_requests, 10);
            assert_eq!(circuit.is_open(), expected_open);
        }
    }

    #[test]
    fn waits_for_minimum_request_count() {
        let mut circuit = CircuitBreakerContext::new(0.5, 60);
        for _ in 0..9 {
            circuit.record_failure();
        }
        assert!(circuit.is_closed());
        circuit.record_failure();
        assert!(circuit.is_open());
    }

    #[test]
    fn recovery_interval_uses_seconds() {
        let mut circuit = CircuitBreakerContext::new(0.5, 60);
        circuit.update_state(CircuitBreakerState::Open);
        let now = Instant::now();
        circuit.stats.last_opened_time = Some(now);
        assert!(!circuit.should_half_open_at(now + Duration::from_secs(59)));
        assert!(circuit.should_half_open_at(now + Duration::from_secs(60)));
        assert!(!circuit.should_half_open_at(now - Duration::from_secs(1)));
        circuit.reset();
        assert!(!circuit.should_half_open());
    }
}
