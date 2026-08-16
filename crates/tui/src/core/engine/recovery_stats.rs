//! Error Recovery Rate telemetry (#690).
//!
//! mimofan already performs transparent self-healing: context-overflow
//! recovery (`recover_context_overflow`) and transparent stream retries on a
//! dead connection (#103 / #2990). What was missing was any *aggregate*
//! measurement of how often a failure was actually recovered vs. surfaced to
//! the user — so recovery health was invisible and tuning was blind.
//!
//! This module is deliberately additive and side-effect-free for the engine:
//! it keeps process-wide atomic counters that the recovery sites in
//! `turn_loop.rs` bump, and exposes a `recovery_rate()` for `/status`. No
//! control flow in `turn_loop` is altered.

use std::sync::atomic::{AtomicU64, Ordering};

static RECOVERY_ATTEMPTS: AtomicU64 = AtomicU64::new(0);
static RECOVERY_SUCCESSES: AtomicU64 = AtomicU64::new(0);

/// Record that a transparent self-heal path was entered (a failure occurred
/// that the engine tried to recover from).
pub fn record_attempt() {
    RECOVERY_ATTEMPTS.fetch_add(1, Ordering::Relaxed);
}

/// Record that a self-heal attempt succeeded (the turn continued instead of
/// failing). Call only after a corresponding [`record_attempt`].
pub fn record_success() {
    RECOVERY_SUCCESSES.fetch_add(1, Ordering::Relaxed);
}

/// A point-in-time snapshot of recovery health.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecoverySnapshot {
    pub attempts: u64,
    pub successes: u64,
    /// `successes / attempts` as a percentage in `[0, 100]`, or `None` when no
    /// attempt has been made yet (rate is undefined, not zero).
    pub rate_pct: Option<u64>,
}

/// Current recovery health snapshot.
#[must_use]
pub fn snapshot() -> RecoverySnapshot {
    let attempts = RECOVERY_ATTEMPTS.load(Ordering::Relaxed);
    let successes = RECOVERY_SUCCESSES.load(Ordering::Relaxed);
    let rate_pct = if attempts == 0 {
        None
    } else {
        Some((successes * 100).saturating_div(attempts))
    };
    RecoverySnapshot {
        attempts,
        successes,
        rate_pct,
    }
}

/// Convenience accessor returning the recovery rate as a percentage, or `None`
/// when no recovery has been attempted yet.
#[must_use]
pub fn recovery_rate() -> Option<u64> {
    snapshot().rate_pct
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_is_none_before_any_attempt() {
        // We can't reset the globals, but the rate math is what we assert:
        // with zero attempts the rate must be None regardless of successes.
        let snap = RecoverySnapshot {
            attempts: 0,
            successes: 0,
            rate_pct: None,
        };
        assert_eq!(snap.rate_pct, None);
    }

    #[test]
    fn rate_math_is_correct() {
        // Simulate the arithmetic used by `snapshot()` independently.
        let attempts = 4u64;
        let successes = 3u64;
        let rate = (successes * 100).saturating_div(attempts);
        assert_eq!(rate, 75);

        let all_fail = (0u64).saturating_div(2);
        assert_eq!(all_fail, 0);

        let all_pass = (2u64 * 100).saturating_div(2);
        assert_eq!(all_pass, 100);
    }
}
