use crate::tools::goal::{GoalStatus, SharedGoalState};

pub(super) fn normalized_goal_objective(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub(super) fn sync_goal_state_from_host(
    goal_state: &SharedGoalState,
    objective: Option<&str>,
    token_budget: Option<u32>,
    status: GoalStatus,
) {
    match goal_state.lock() {
        Ok(mut state) => state.sync_from_host_status(objective, token_budget, status),
        Err(err) => tracing::warn!("goal state lock poisoned while syncing host goal: {err}"),
    }
}

pub(super) fn goal_objective_for_prompt(
    configured_goal: Option<&str>,
    goal_state: &SharedGoalState,
) -> Option<String> {
    match goal_state.lock() {
        Ok(state) => {
            if let Some(objective) = state.objective() {
                // Preserve original behavior: return None (not fallback) when
                // objective exists but goal is inactive.
                return state.is_active().then(|| objective.to_string());
            }
        }
        Err(err) => tracing::warn!("goal state lock poisoned while building prompt: {err}"),
    }
    normalized_goal_objective(configured_goal)
}

/// The full goal contract for prompt injection: objective plus the completion
/// criteria (from the goal's `completion_verification.check`) and the progress
/// checklist. Returns `None` when there is no active goal, so callers keep the
/// prior "no goal → no injection" behavior. The lock guard is dropped before
/// any await (see `ARCHITECTURE_STABILITY.md` §8.3).
pub(super) struct GoalContract {
    pub objective: String,
    pub completion_check: Option<String>,
    pub progress_checklist: Option<String>,
}

pub(super) fn goal_contract_for_prompt(goal_state: &SharedGoalState) -> Option<GoalContract> {
    match goal_state.lock() {
        Ok(state) => {
            let objective = state.objective()?;
            if !state.is_active() {
                return None;
            }
            Some(GoalContract {
                objective: objective.to_string(),
                completion_check: state
                    .completion_verification()
                    .filter(|v| !v.check.trim().is_empty())
                    .map(|v| v.check.clone()),
                progress_checklist: state
                    .progress_checklist()
                    .filter(|p| !p.trim().is_empty())
                    .map(str::to_string),
            })
        }
        Err(err) => {
            tracing::warn!("goal state lock poisoned while building goal contract: {err}");
            None
        }
    }
}
