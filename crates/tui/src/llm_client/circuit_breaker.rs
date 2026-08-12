//! Circuit breaker for provider fault isolation.
//!
//! A [`CircuitBreaker`] is a pure, deterministic state machine used to fail
//! fast when an upstream LLM provider is unhealthy. It is intentionally free
//! of any network, clock, or randomness dependency so it can be unit tested
//! deterministically.
//!
//! # States
//!
//! - `Closed`: healthy; requests flow through. Failures are counted.
//! - `Open`: unhealthy; requests are rejected immediately without being sent.
//!   After `cooldown` elapses the breaker transitions to `HalfOpen`.
//! - `HalfOpen`: a probationary state; a single probe request is allowed. A
//!   success closes the breaker, a failure re-opens it.

use std::time::{Duration, Instant};

/// The three states of a [`CircuitBreaker`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitBreakerState {
    /// Healthy; requests flow through and failures are counted.
    Closed,
    /// Unhealthy; requests are rejected immediately.
    Open,
    /// Probationary; a single probe request is permitted.
    HalfOpen,
}

/// A circuit breaker that isolates an unhealthy upstream provider.
///
/// Instances are cheap to construct and share (`Clone` + `Send + Sync`) so they
/// can be owned by caller code and threaded through retry loops. The breaker
/// does not itself perform requests; callers consult [`CircuitBreaker::can_attempt`]
/// before issuing a request and report outcomes via [`CircuitBreaker::record_success`]
/// / [`CircuitBreaker::record_failure`].
#[derive(Debug, Clone)]
pub struct CircuitBreaker {
    /// Consecutive failures allowed before the breaker trips to `Open`.
    failure_threshold: u32,
    /// How long the breaker stays `Open` before probing in `HalfOpen`.
    cooldown: Duration,
    /// Current state.
    state: CircuitBreakerState,
    /// Consecutive failure count in the `Closed` state.
    failure_count: u32,
    /// Instant at which the breaker tripped `Open`, if it is currently open.
    opened_at: Option<Instant>,
}

impl CircuitBreaker {
    /// Creates a new closed breaker with the given threshold and cooldown.
    #[must_use]
    pub fn new(failure_threshold: u32, cooldown: Duration) -> Self {
        Self {
            failure_threshold,
            cooldown,
            state: CircuitBreakerState::Closed,
            failure_count: 0,
            opened_at: None,
        }
    }

    /// Returns the current state.
    #[must_use]
    pub fn state(&self) -> CircuitBreakerState {
        self.state
    }

    /// Returns `true` when the breaker is currently `Open` (and not yet ready
    /// to probe). This is the complement of [`CircuitBreaker::can_attempt`] for
    /// the rejecting case.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.state == CircuitBreakerState::Open
    }

    /// Returns `true` if a request may be attempted right now.
    ///
    /// - `Closed`: always allowed.
    /// - `HalfOpen`: allowed (only one probe should be issued per half-open
    ///   window; callers are expected to issue a single probe).
    /// - `Open`: allowed only once `cooldown` has elapsed, at which point the
    ///   breaker transitions to `HalfOpen` and the probe is permitted.
    #[must_use]
    pub fn can_attempt(&self, now: Instant) -> bool {
        match self.state {
            CircuitBreakerState::Closed | CircuitBreakerState::HalfOpen => true,
            CircuitBreakerState::Open => match self.opened_at {
                Some(opened_at) if now.saturating_duration_since(opened_at) >= self.cooldown => {
                    true
                }
                _ => false,
            },
        }
    }

    /// Records a successful request.
    ///
    /// Any success (whether in `Closed`, `HalfOpen`, or a stale `Open` probe)
    /// resets the failure count and closes the breaker.
    pub fn record_success(&mut self) {
        self.failure_count = 0;
        self.opened_at = None;
        self.state = CircuitBreakerState::Closed;
    }

    /// Records a failed request.
    ///
    /// - In `Closed`: increments the failure count; once it reaches
    ///   `failure_threshold` the breaker trips to `Open`.
    /// - In `HalfOpen`: any failure re-opens the breaker immediately.
    /// - In `Open`: failures while open are ignored (the breaker is already
    ///   open); `opened_at` is untouched so the cooldown clock keeps running.
    pub fn record_failure(&mut self, now: Instant) {
        match self.state {
            CircuitBreakerState::Closed => {
                self.failure_count += 1;
                if self.failure_count >= self.failure_threshold {
                    self.state = CircuitBreakerState::Open;
                    self.opened_at = Some(now);
                }
            }
            CircuitBreakerState::HalfOpen => {
                self.state = CircuitBreakerState::Open;
                self.opened_at = Some(now);
            }
            CircuitBreakerState::Open => {}
        }
    }

    /// Advances the breaker's internal state based on the current time, without
    /// recording an outcome. This transitions `Open` -> `HalfOpen` once the
    /// cooldown has elapsed. Returns the state after the transition.
    ///
    /// Callers should invoke this before deciding whether to attempt a request,
    /// typically right before calling [`CircuitBreaker::can_attempt`].
    pub fn tick(&mut self, now: Instant) -> CircuitBreakerState {
        if self.state == CircuitBreakerState::Open {
            if let Some(opened_at) = self.opened_at {
                if now.saturating_duration_since(opened_at) >= self.cooldown {
                    self.state = CircuitBreakerState::HalfOpen;
                }
            }
        }
        self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closed_breaker_allows_attempts() {
        let now = Instant::now();
        let cb = CircuitBreaker::new(3, Duration::from_secs(10));
        assert_eq!(cb.state(), CircuitBreakerState::Closed);
        assert!(!cb.is_open());
        assert!(cb.can_attempt(now));
    }

    #[test]
    fn consecutive_failures_trip_to_open() {
        let base = Instant::now();
        let mut cb = CircuitBreaker::new(3, Duration::from_secs(10));
        cb.record_failure(base); // 1
        assert_eq!(cb.state(), CircuitBreakerState::Closed);
        cb.record_failure(base + Duration::from_secs(1)); // 2
        assert_eq!(cb.state(), CircuitBreakerState::Closed);
        cb.record_failure(base + Duration::from_secs(2)); // 3 -> tripped
        assert_eq!(cb.state(), CircuitBreakerState::Open);
        assert!(cb.is_open());
    }

    #[test]
    fn success_resets_failure_count() {
        let base = Instant::now();
        let mut cb = CircuitBreaker::new(3, Duration::from_secs(10));
        cb.record_failure(base);
        cb.record_failure(base);
        cb.record_success();
        assert_eq!(cb.state(), CircuitBreakerState::Closed);
        assert_eq!(cb.failure_count, 0);
        // Need a full new threshold of failures to trip again.
        cb.record_failure(base);
        assert_eq!(cb.state(), CircuitBreakerState::Closed);
    }

    #[test]
    fn open_breaker_rejects_before_cooldown() {
        let base = Instant::now();
        let mut cb = CircuitBreaker::new(1, Duration::from_secs(10));
        cb.record_failure(base);
        assert!(cb.is_open());
        // Still within cooldown.
        assert!(!cb.can_attempt(base + Duration::from_secs(9)));
    }

    #[test]
    fn open_breaker_probes_after_cooldown() {
        let base = Instant::now();
        let mut cb = CircuitBreaker::new(1, Duration::from_secs(10));
        cb.record_failure(base);
        let probe_time = base + Duration::from_secs(11);
        assert!(cb.can_attempt(probe_time));
        // The probe is allowed; advance state explicitly via tick.
        assert_eq!(cb.tick(probe_time), CircuitBreakerState::HalfOpen);
        assert!(cb.can_attempt(probe_time));
    }

    #[test]
    fn half_open_success_closes_breaker() {
        let base = Instant::now();
        let mut cb = CircuitBreaker::new(1, Duration::from_secs(10));
        cb.record_failure(base);
        let probe_time = base + Duration::from_secs(11);
        cb.tick(probe_time);
        assert_eq!(cb.state(), CircuitBreakerState::HalfOpen);
        cb.record_success();
        assert_eq!(cb.state(), CircuitBreakerState::Closed);
        assert!(!cb.is_open());
        assert_eq!(cb.failure_count, 0);
    }

    #[test]
    fn half_open_failure_reopens_breaker() {
        let base = Instant::now();
        let mut cb = CircuitBreaker::new(1, Duration::from_secs(10));
        cb.record_failure(base);
        let probe_time = base + Duration::from_secs(11);
        cb.tick(probe_time);
        assert_eq!(cb.state(), CircuitBreakerState::HalfOpen);
        // A probe failure while half-open re-opens immediately.
        cb.record_failure(probe_time);
        assert_eq!(cb.state(), CircuitBreakerState::Open);
        assert!(cb.is_open());
        // Still inside the new cooldown window.
        assert!(!cb.can_attempt(probe_time + Duration::from_secs(1)));
    }
}
