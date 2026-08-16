//! Unified agent lifecycle state machine (#864) + stalled detection (#866).
//!
//! These types are intentionally decoupled from the `SubAgentManager`'s
//! durable worker records: the lifecycle tracker captures the coarse,
//! queryable lifecycle of a spawned agent from the orchestrator's point of
//! view (Idle → Working → Blocked → Done), while the stalled detector
//! watches the *cadence* of state changes (a cheap liveness signal that is
//! distinct from a long wall-clock timeout).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Condition used by [`crate::tools::subagent::SubAgentManager::wait_until`]
/// (#865).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitCond {
    /// Wait until the agent is `Blocked` (parked on human input / an
    /// approval gate) — NOT merely running.
    Blocked,
    /// Wait until the agent is terminal (`Done`).
    Done,
}

/// The coarse lifecycle of a spawned sub-agent, from the orchestrator's
/// perspective (#864).
///
/// This is deliberately narrower than `SubAgentStatus`: it answers the
/// question "what is the child doing right now from my point of view?" and
/// is cheap to query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentLifecycleState {
    /// Spawned but not yet started executing its first step.
    Idle,
    /// Actively executing work. This is NOT "blocked" — it is making
    /// progress (or at least is not waiting on external input).
    Working,
    /// Paused, waiting on human input or an approval gate. Crucially this
    /// does NOT mean merely "running"; it means the agent cannot proceed
    /// until something external resolves (a prompt answer, an approval).
    Blocked(String),
    /// Reached a terminal state (completed / failed / cancelled).
    Done,
}

impl AgentLifecycleState {
    /// Whether this state is terminal (no further transitions expected).
    #[must_use]
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Done)
    }

    /// Whether the agent is parked waiting on external input/approval.
    #[must_use]
    pub fn is_blocked(&self) -> bool {
        matches!(self, Self::Blocked(_))
    }

    /// Human-readable slug, handy for logs / sentinel payloads.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Working => "working",
            Self::Blocked(_) => "blocked",
            Self::Done => "done",
        }
    }
}

/// A single tracked agent entry inside the [`LifecycleTracker`].
#[derive(Debug, Clone)]
struct TrackedAgent {
    state: AgentLifecycleState,
    /// Wall-clock time of the last state transition. Used by the stalled
    /// detector to distinguish "no progress within a short window" from a
    /// genuinely long (but healthy) task.
    last_change: Instant,
}

/// Thread-safe registry of agent lifecycle states (#864).
///
/// Keyed by the same `String` agent id used everywhere else in the manager
/// (`SubAgent.id`). Wrapped in `Arc<Mutex<...>>` so it can be cloned into
/// async task contexts and queried without holding the manager's `RwLock`.
#[derive(Debug, Clone, Default)]
pub struct LifecycleTracker {
    inner: Arc<Mutex<HashMap<String, TrackedAgent>>>,
}

impl LifecycleTracker {
    /// Create an empty tracker.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Begin tracking `id` in the [`AgentLifecycleState::Idle`] state.
    pub fn register(&self, id: &str) {
        let mut guard = self.inner.lock().expect("lifecycle tracker poisoned");
        guard.insert(
            id.to_string(),
            TrackedAgent {
                state: AgentLifecycleState::Idle,
                last_change: Instant::now(),
            },
        );
    }

    /// Transition `id` to `state`, bumping the last-change timestamp.
    /// Registering a first state via this method also creates the entry, so
    /// callers may `set` directly without a prior `register`.
    pub fn set(&self, id: &str, state: AgentLifecycleState) {
        let mut guard = self.inner.lock().expect("lifecycle tracker poisoned");
        guard.insert(
            id.to_string(),
            TrackedAgent {
                state,
                last_change: Instant::now(),
            },
        );
    }

    /// Get the current state of `id`, if tracked.
    #[must_use]
    pub fn state(&self, id: &str) -> Option<AgentLifecycleState> {
        let guard = self.inner.lock().expect("lifecycle tracker poisoned");
        guard.get(id).map(|entry| entry.state.clone())
    }

    /// Snapshot of every tracked agent's id → state.
    #[must_use]
    pub fn all_states(&self) -> HashMap<String, AgentLifecycleState> {
        let guard = self.inner.lock().expect("lifecycle tracker poisoned");
        guard
            .iter()
            .map(|(id, entry)| (id.clone(), entry.state.clone()))
            .collect()
    }

    /// Wall-clock duration since `id`'s last state change, if tracked.
    #[must_use]
    pub fn since_last_change(&self, id: &str) -> Option<Duration> {
        let guard = self.inner.lock().expect("lifecycle tracker poisoned");
        guard.get(id).map(|entry| entry.last_change.elapsed())
    }

    /// Drop tracking for `id` (e.g. on eviction/cleanup).
    pub fn remove(&self, id: &str) {
        let mut guard = self.inner.lock().expect("lifecycle tracker poisoned");
        guard.remove(id);
    }
}

/// Cheap liveness guard (#866).
///
/// Given a spawned agent, `assert_not_stalled` fires if NO lifecycle-state
/// change has occurred within `window` after a prompt/instruction was
/// issued — distinct from a long wall-clock timeout. A long-running but
/// *progressing* agent (multiple transitions) is never flagged; only a
/// silent one is.
#[derive(Debug, Clone, Default)]
pub struct StalledDetector {
    tracker: LifecycleTracker,
}

impl StalledDetector {
    /// Build a detector that watches the given tracker.
    #[must_use]
    pub fn new(tracker: LifecycleTracker) -> Self {
        Self { tracker }
    }

    /// Fail fast if `id` has not changed lifecycle state within `window`.
    ///
    /// Returns `Ok(())` when the agent is still making lifecycle progress
    /// (its last state change is more recent than `window`). Returns
    /// `Err` (with the elapsed-since-last-change duration) when the agent
    /// has been silent past `window` — i.e. it is stalled.
    pub fn assert_not_stalled(&self, id: &str, window: Duration) -> Result<(), Duration> {
        match self.tracker.since_last_change(id) {
            // Not tracked at all: treat as stalled (nothing to observe).
            None => Err(window),
            Some(elapsed) if elapsed >= window => Err(elapsed),
            Some(_) => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_transitions_idle_working_blocked_done() {
        let tracker = LifecycleTracker::new();
        tracker.register("a1");
        assert_eq!(tracker.state("a1"), Some(AgentLifecycleState::Idle));

        tracker.set("a1", AgentLifecycleState::Working);
        assert_eq!(tracker.state("a1"), Some(AgentLifecycleState::Working));

        tracker.set(
            "a1",
            AgentLifecycleState::Blocked("needs_approval".to_string()),
        );
        match tracker.state("a1") {
            Some(AgentLifecycleState::Blocked(reason)) => assert_eq!(reason, "needs_approval"),
            other => panic!("expected Blocked, got {other:?}"),
        }

        tracker.set("a1", AgentLifecycleState::Done);
        assert_eq!(tracker.state("a1"), Some(AgentLifecycleState::Done));
        assert!(tracker.state("a1").unwrap().is_terminal());
    }

    #[test]
    fn all_states_snapshot() {
        let tracker = LifecycleTracker::new();
        tracker.register("x");
        tracker.set("y", AgentLifecycleState::Working);
        let all = tracker.all_states();
        assert_eq!(all.len(), 2);
        assert_eq!(all["x"], AgentLifecycleState::Idle);
        assert_eq!(all["y"], AgentLifecycleState::Working);
    }

    #[test]
    fn stalled_detector_fires_on_no_change() {
        let tracker = LifecycleTracker::new();
        // register then immediately check with a tiny window: enough time
        // passes in the test so the last-change is older than `window`.
        tracker.register("silent");
        std::thread::sleep(Duration::from_millis(20));
        let detector = StalledDetector::new(tracker);
        let result = detector.assert_not_stalled("silent", Duration::from_millis(5));
        assert!(result.is_err(), "expected stalled detection to fire");
    }

    #[test]
    fn stalled_detector_ok_on_recent_change() {
        let tracker = LifecycleTracker::new();
        tracker.register("active");
        // Immediately re-set to reset the timestamp, then check with a wide window.
        tracker.set("active", AgentLifecycleState::Working);
        let detector = StalledDetector::new(tracker);
        let result = detector.assert_not_stalled("active", Duration::from_millis(50));
        assert!(result.is_ok(), "recent change must not be flagged stalled");
    }
}
