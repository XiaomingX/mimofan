//! Turn-loop interceptor seam (W3, issue #836).
//!
//! A `TurnInterceptor` lets plugins/eval harnesses hook into the existing
//! agent loop *without* rewriting any of its logic. Every method has a
//! default no-op implementation, so a turn with no registered interceptors
//! behaves exactly as before.
//!
//! The seam wraps three existing points in `turn_loop.rs`:
//!   - `pre_step`   before each `rx_steer` drain,
//!   - `request`    on the `MessageRequest` built for a provider call,
//!   - `post_step`  after a step completes,
//!   - `turn_stopping` consulted alongside the existing stop decision for
//!     plan tools — returning `Some(true)` forces the turn to stop early
//!     (OR-ed with the existing logic); `None` defers to existing behavior.

use crate::models::MessageRequest;

/// A hook into the agent's turn loop.
///
/// Trait objects are `Send + Sync` so interceptors can be shared across the
/// engine and (future) plugin threads.
pub trait TurnInterceptor: Send + Sync {
    /// Called at the start of every loop iteration, before the steering
    /// channel is drained. `workspace` is the session root path.
    fn pre_step(&self, _workspace: &str) {}

    /// Called with the provider `MessageRequest` right after it is built,
    /// before it is sent. Implementations may mutate `req` (e.g. inject
    /// instructions or redact tool choices).
    fn request(&self, _req: &mut MessageRequest) {}

    /// Called after a step's tool outcomes have been processed. `turn` is the
    /// current engine turn counter.
    fn post_step(&self, _turn: u64) {}

    /// Consulted when deciding whether to stop the turn.
    ///
    /// Returns:
    ///   - `None`      → defer to the existing stop logic,
    ///   - `Some(true)`  → force-stop the turn (OR-ed with existing logic),
    ///   - `Some(false)` → explicitly do not override (still OR-ed, so a
    ///                     `true` from another interceptor / existing logic wins).
    fn turn_stopping(&self, _turn: u64) -> Option<bool> {
        None
    }
}

/// Pure combine helper: OR the existing stop decision with any interceptor
/// that wants to force a stop. Kept side-effect-free so it can be unit-tested
/// without constructing a full `Engine`.
pub fn combine_stop(
    existing: bool,
    interceptors: &[Box<dyn TurnInterceptor>],
    turn: u64,
) -> bool {
    existing || interceptors.iter().any(|ic| ic.turn_stopping(turn) == Some(true))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dummy interceptor used to verify the seam invokes `turn_stopping` and
    /// that `combine_stop` honors a `Some(true)` override.
    struct ForceStopInterceptor {
        calls: std::sync::Arc<std::sync::atomic::AtomicU64>,
    }

    impl TurnInterceptor for ForceStopInterceptor {
        fn turn_stopping(&self, _turn: u64) -> Option<bool> {
            self.calls
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Some(true)
        }
    }

    /// No-op interceptor — preserves existing behavior.
    struct NoopInterceptor;

    impl TurnInterceptor for NoopInterceptor {}

    fn boxed(ic: impl TurnInterceptor + 'static) -> Box<dyn TurnInterceptor> {
        Box::new(ic)
    }

    #[test]
    fn combine_stop_defers_when_no_interceptor() {
        let interceptors: Vec<Box<dyn TurnInterceptor>> = vec![];
        assert!(!combine_stop(false, &interceptors, 1));
        assert!(combine_stop(true, &interceptors, 1));
    }

    #[test]
    fn combine_stop_forces_stop_on_some_true() {
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let interceptors: Vec<Box<dyn TurnInterceptor>> =
            vec![boxed(ForceStopInterceptor { calls: calls.clone() })];
        // existing=false, but interceptor overrides to stop.
        assert!(combine_stop(false, &interceptors, 3));
        // existing=true is still true.
        assert!(combine_stop(true, &interceptors, 3));
        // `turn_stopping` was consulted at least once (`.any` short-circuits
        // after the first `Some(true)`, so it is called exactly once here).
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn combine_stop_noop_interceptor_does_not_force_stop() {
        let interceptors: Vec<Box<dyn TurnInterceptor>> = vec![boxed(NoopInterceptor)];
        assert!(!combine_stop(false, &interceptors, 1));
    }

    #[test]
    fn combine_stop_or_semantics_across_multiple_interceptors() {
        // A no-op plus a force-stop → still stops.
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let interceptors: Vec<Box<dyn TurnInterceptor>> = vec![
            boxed(NoopInterceptor),
            boxed(ForceStopInterceptor { calls }),
        ];
        assert!(combine_stop(false, &interceptors, 0));
    }
}
