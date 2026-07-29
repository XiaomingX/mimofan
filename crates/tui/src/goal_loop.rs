//! Goal loop orchestrator — the persistent-objective control layer (#3215, and
//! its lineage #891 / #1976 / #2058 / #2029).
//!
//! This is the **WhaleFlow goal layer**: the decision core that turns a one-shot
//! `/goal` into a persistent work loop. Given the durable goal status, the
//! accumulated usage (from the per-goal accounting wired in `crates/state`
//! `record_thread_goal_usage`), and a budget, it decides whether to **continue**
//! (re-dispatch another worker turn toward the objective) or **stop** with a
//! terminal status. It is the orchestrator in the WhaleFlow≈ultracode mapping —
//! the loop that fans work out to workers (`worker_profile`) and verifies before
//! committing.
//!
//! Scope: **decision logic + types**. The engine (`core/engine.rs`) reads the
//! `SharedGoalState` snapshot after each turn and calls `decide_continuation`
//! to decide whether to re-dispatch. There is **no continuation cap** — a goal
//! runs until the model self-reports complete/blocked, the user pauses or
//! clears, or an optional token/time budget is exhausted. This matches how a
//! persistent objective should feel: "until done," not "until N turns."

/// Default safety cap on continuations per goal run.  Prevents unbounded loops
/// when the model fails to self-report completion.  Can be overridden via
/// `GoalBudget::max_continuations`.
pub const DEFAULT_MAX_CONTINUATIONS: u32 = 50;

/// Terminal or active state of a persistent goal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoalRunStatus {
    /// Still working toward the objective.
    Active,
    /// The objective was achieved (the model self-reported done and, ideally, a
    /// verifier confirmed — see `GoalGate`).
    Completed,
    /// The model reported it is blocked and needs the user.
    Blocked,
}

/// Why the loop stopped, for a terminal decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    /// Objective achieved.
    Completed,
    /// Model reported blocked.
    Blocked,
    /// Token budget exhausted.
    TokenBudget,
    /// Wall-clock budget exhausted.
    TimeBudget,
    /// Continuation circuit-breaker tripped (too many continuations without a
    /// terminal signal). Retained for API completeness; the current loop has no
    /// continuation cap, so this variant is not constructed by
    /// `decide_continuation`.
    ContinuationLimit,
}

/// Accumulated, durable progress for a goal run. Mirrors the fields wired by
/// `crates/state` `record_thread_goal_usage` (tokens_used / time_used_seconds)
/// plus a continuation counter the loop maintains.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GoalProgress {
    pub tokens_used: u64,
    pub time_used_seconds: u64,
    pub continuations: u32,
}

/// The bound on a goal run. `None` fields mean unbounded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GoalBudget {
    pub token_budget: Option<u64>,
    pub time_budget_seconds: Option<u64>,
    /// Maximum number of continuations before the loop stops.  `None` means
    /// unbounded — the loop runs until the model self-reports complete/blocked,
    /// the user pauses/clears, or an optional budget is exhausted.
    pub max_continuations: Option<u32>,
}

impl GoalBudget {
    /// Fully unbounded — no token, time, or continuation cap. The only stops
    /// are a terminal model status (complete/blocked) or an explicit user
    /// pause/clear.
    pub const fn unbounded() -> Self {
        Self {
            token_budget: None,
            time_budget_seconds: None,
            max_continuations: None,
        }
    }

    /// A token budget only — the loop runs until the model is done or the
    /// token budget is exhausted.
    pub const fn with_token_budget(token_budget: u64) -> Self {
        Self {
            token_budget: Some(token_budget),
            time_budget_seconds: None,
            max_continuations: None,
        }
    }
}

/// The decision the loop makes after each worker turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContinuationDecision {
    /// Re-dispatch another turn toward the objective.
    Continue,
    /// Stop; the goal run is terminal.
    Stop(StopReason),
}

/// Decide whether a persistent goal run should continue after a turn.
///
/// Precedence (most authoritative first):
/// 1. A terminal model status (Completed / Blocked) ends the run.
/// 2. An optional continuation cap, if exhausted, ends the run.
/// 3. An optional token or time budget, if exhausted, ends the run.
/// 4. Otherwise continue.
#[must_use]
pub fn decide_continuation(
    status: GoalRunStatus,
    progress: GoalProgress,
    budget: GoalBudget,
) -> ContinuationDecision {
    // 1. Terminal model signal wins.
    match status {
        GoalRunStatus::Completed => return ContinuationDecision::Stop(StopReason::Completed),
        GoalRunStatus::Blocked => return ContinuationDecision::Stop(StopReason::Blocked),
        GoalRunStatus::Active => {}
    }

    // 2. Continuation cap.
    if let Some(max) = budget.max_continuations
        && progress.continuations >= max
    {
        return ContinuationDecision::Stop(StopReason::ContinuationLimit);
    }

    // 3. Optional budget.
    if let Some(tokens) = budget.token_budget
        && progress.tokens_used >= tokens
    {
        return ContinuationDecision::Stop(StopReason::TokenBudget);
    }
    if let Some(secs) = budget.time_budget_seconds
        && progress.time_used_seconds >= secs
    {
        return ContinuationDecision::Stop(StopReason::TimeBudget);
    }

    // 4. Keep going.
    ContinuationDecision::Continue
}

#[cfg(test)]
mod tests {}
