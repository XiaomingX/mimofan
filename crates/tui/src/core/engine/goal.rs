use crate::tools::goal::{GoalStatus, SharedGoalQueue};

pub(super) fn normalized_goal_objective(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub(super) fn sync_goal_state_from_host(
    goal_queue: &SharedGoalQueue,
    objective: Option<&str>,
    token_budget: Option<u32>,
    status: GoalStatus,
) {
    match goal_queue.lock() {
        Ok(mut queue) => {
            // 将 host 目标落到「active」条目（若没有则入队一个新条目并提升）。
            queue.sync_active_from_host(objective, token_budget, status);
        }
        Err(err) => tracing::warn!("goal queue lock poisoned while syncing host goal: {err}"),
    }
}

pub(super) fn goal_objective_for_prompt(
    configured_goal: Option<&str>,
    goal_queue: &SharedGoalQueue,
) -> Option<String> {
    match goal_queue.lock() {
        Ok(queue) => {
            if let Some(snapshot) = queue.active_snapshot() {
                if let Some(objective) = snapshot.objective.as_ref() {
                    // 仅当 active goal 仍在进行中才注入（与旧 is_active 行为一致）。
                    if snapshot.status == GoalStatus::Active.as_str() {
                        return Some(objective.clone());
                    }
                }
            }
        }
        Err(err) => tracing::warn!("goal queue lock poisoned while building prompt: {err}"),
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

pub(super) fn goal_contract_for_prompt(goal_queue: &SharedGoalQueue) -> Option<GoalContract> {
    match goal_queue.lock() {
        Ok(queue) => {
            let snapshot = queue.active_snapshot()?;
            let objective = snapshot.objective?;
            if snapshot.status != GoalStatus::Active.as_str() {
                return None;
            }
            Some(GoalContract {
                objective,
                completion_check: snapshot
                    .completion_verification
                    .as_ref()
                    .filter(|v| !v.check.trim().is_empty())
                    .map(|v| v.check.clone()),
                progress_checklist: snapshot.progress_checklist.filter(|p| !p.trim().is_empty()),
            })
        }
        Err(err) => {
            tracing::warn!("goal queue lock poisoned while building goal contract: {err}");
            None
        }
    }
}
