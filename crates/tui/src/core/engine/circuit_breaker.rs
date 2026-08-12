//! Cross-provider circuit breaker (#619).
//!
//! The existing provider failover path (`advance_fallback`) only ever walks a
//! fallback *list*; it never *trips* a provider after repeated failures, so a
//! provider that is consistently erroring still gets retried every turn. This
//! module adds a small, dependency-free [`CircuitBreaker`] that trips open
//! after `failure_threshold` consecutive failures, stays open for a cooldown,
//! then half-opens for a single probe before closing again.
//!
//! It is intentionally generic over a provider/endpoint key (`String`) and has
//! no IO of its own — callers record outcomes and query `allow_request`.

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Default consecutive-failure count that trips the breaker open.
pub const DEFAULT_FAILURE_THRESHOLD: u32 = 3;
/// Default cooldown before a half-open probe is permitted.
pub const DEFAULT_COOLDOWN: Duration = Duration::from_secs(30);

/// Per-key breaker state.
#[derive(Debug, Clone)]
struct BreakerState {
    failures: u32,
    opened_at: Option<Instant>,
    half_open_probe: bool,
}

impl BreakerState {
    fn new() -> Self {
        Self {
            failures: 0,
            opened_at: None,
            half_open_probe: false,
        }
    }
}

/// A circuit breaker registry keyed by provider/endpoint id.
///
/// `now` is injectable (via [`CircuitBreaker::record_outcome_with`]) for
/// deterministic tests; the convenience [`CircuitBreaker::record_outcome`] uses
/// `Instant::now()`.
#[derive(Default)]
pub struct CircuitBreaker {
    states: HashMap<String, BreakerState>,
    failure_threshold: u32,
    cooldown: Duration,
}

impl CircuitBreaker {
    /// Create a breaker with default threshold/cooldown.
    pub fn new() -> Self {
        Self {
            states: HashMap::new(),
            failure_threshold: DEFAULT_FAILURE_THRESHOLD,
            cooldown: DEFAULT_COOLDOWN,
        }
    }

    /// Whether a request to `key` is currently permitted.
    ///
    /// - Closed (healthy): always allowed.
    /// - Open (tripped, cooldown not elapsed): denied.
    /// - Open but cooldown elapsed: allowed *once* as a half-open probe; this
    ///   call consumes the probe (flips `half_open_probe`) so a second call at
    ///   the same instant is denied until the next outcome is recorded.
    pub fn allow_request(&mut self, key: &str, now: Instant) -> bool {
        match self.states.get_mut(key) {
            None => true,
            Some(s) => {
                if s.failures < self.failure_threshold {
                    return true; // closed
                }
                match s.opened_at {
                    None => true,
                    Some(opened) if now.duration_since(opened) >= self.cooldown => {
                        // Half-open: permit exactly one probe, then consume it.
                        if s.half_open_probe {
                            false
                        } else {
                            s.half_open_probe = true;
                            true
                        }
                    }
                    Some(_) => false, // still cooling down
                }
            }
        }
    }

    /// Record a success: resets the failure count and closes the breaker.
    pub fn record_success(&mut self, key: &str) {
        self.states.remove(key);
    }

    /// Record a failure at `now`; trips open after the threshold.
    pub fn record_failure(&mut self, key: &str, now: Instant) {
        let s = self.states.entry(key.to_string()).or_insert_with(BreakerState::new);
        s.failures += 1;
        if s.failures >= self.failure_threshold {
            s.opened_at = Some(now);
            s.half_open_probe = false;
        }
    }

    /// Test-friendly variant: records using an explicit clock.
    pub fn record_outcome_with(&mut self, key: &str, ok: bool, now: Instant) {
        if ok {
            self.record_success(key);
        } else {
            self.record_failure(key, now);
        }
    }

    /// Convenience: record using the real clock.
    pub fn record_outcome(&mut self, key: &str, ok: bool) {
        self.record_outcome_with(key, ok, Instant::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closed_allows_until_threshold() {
        let mut cb = CircuitBreaker::new();
        let now = Instant::now();
        assert!(cb.allow_request("p1", now));
        cb.record_failure("p1", now);
        cb.record_failure("p1", now);
        assert!(cb.allow_request("p1", now), "still under threshold");
        cb.record_failure("p1", now); // 3rd -> trips
        assert!(!cb.allow_request("p1", now), "tripped open");
    }

    #[test]
    fn success_resets() {
        let mut cb = CircuitBreaker::new();
        let now = Instant::now();
        for _ in 0..3 {
            cb.record_failure("p1", now);
        }
        assert!(!cb.allow_request("p1", now));
        cb.record_success("p1");
        assert!(cb.allow_request("p1", now), "recovered after success");
    }

    #[test]
    fn half_open_probe_after_cooldown() {
        let mut cb = CircuitBreaker::new();
        let t0 = Instant::now();
        for _ in 0..3 {
            cb.record_failure("p1", t0);
        }
        assert!(!cb.allow_request("p1", t0));
        // Before cooldown elapses: still denied.
        let t1 = t0 + Duration::from_secs(10);
        assert!(!cb.allow_request("p1", t1));
        // After cooldown: one half-open probe allowed.
        let t2 = t0 + Duration::from_secs(31);
        assert!(cb.allow_request("p1", t2), "half-open probe permitted");
        // A second request at the same instant is denied (probe is single).
        assert!(!cb.allow_request("p1", t2), "only one probe");
    }
}
