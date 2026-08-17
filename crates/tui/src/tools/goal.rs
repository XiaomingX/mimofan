//! Goal tools for the model-visible LLM-as-judge loop.
//!
//! The TUI already has a `/goal` command and passes its objective into the
//! engine prompt. This module keeps the runtime slice separate: a small
//! session-scoped state object plus tools the model can use to inspect and
//! close out that state.
//!
//! 目标管理的纯逻辑（`GoalState` / `GoalQueue` / 快照类型 / 调度算法）已下沉到
//! 独立的 `mimofan-goal-core` crate（不依赖 TUI）。本模块只保留与 TUI 耦合的
//! 部分：工具实现、独立的完成评审门（`independent_judge`，依赖 `crate::reviewer`）、
//! 以及续跑 prompt 的 base 文案注入（`crate::prompts::GOAL_CONTINUATION_PROMPT`）。

pub use mimofan_goal_core::*;

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::prompts::GOAL_CONTINUATION_PROMPT;
use crate::reviewer::{self, ClaimForReview, EvidenceStrength, ReviewVerdict};
use crate::tools::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec, required_str,
};

/// Render the continuation prompt injected when a goal is still active after a
/// turn. Thin wrapper over `goal_core::render_continuation_prompt` that supplies
/// the TUI's continuation instruction text (`GOAL_CONTINUATION_PROMPT`).
#[must_use]
pub fn render_continuation_prompt(
    snapshot: &GoalSnapshot,
    continuation_index: u32,
    stop_condition: Option<&str>,
) -> String {
    mimofan_goal_core::render_goal_continuation_prompt(
        snapshot,
        continuation_index,
        stop_condition,
        GOAL_CONTINUATION_PROMPT,
    )
}

fn lock_goal_queue(
    state: &SharedGoalQueue,
) -> Result<std::sync::MutexGuard<'_, GoalQueue>, ToolError> {
    state
        .lock()
        .map_err(|_| ToolError::execution_failed("goal queue lock poisoned"))
}

fn parse_token_budget(input: &Value) -> Result<Option<u32>, ToolError> {
    let Some(raw) = input.get("token_budget") else {
        return Ok(None);
    };
    if raw.is_null() {
        return Ok(None);
    }
    let Some(value) = raw.as_u64() else {
        return Err(ToolError::invalid_input(
            "token_budget must be a non-negative integer",
        ));
    };
    u32::try_from(value)
        .map(Some)
        .map_err(|_| ToolError::invalid_input("token_budget is too large"))
}

fn parse_completion_verification(input: &Value) -> Result<GoalCompletionVerification, ToolError> {
    let Some(raw) = input.get("verification") else {
        return Err(ToolError::invalid_input(
            "verification is required when status is complete; run a verifier/check and pass verification: {status, check, summary}",
        ));
    };
    let verification: GoalCompletionVerification = serde_json::from_value(raw.clone())
        .map_err(|err| ToolError::invalid_input(format!("invalid verification: {err}")))?;
    if verification.status.trim() != "passed" {
        return Err(ToolError::invalid_input(
            "verification.status must be 'passed' before goal_update can mark a goal complete",
        ));
    }
    if verification.check.trim().is_empty() {
        return Err(ToolError::invalid_input("verification.check is required"));
    }
    if verification.summary.trim().is_empty() {
        return Err(ToolError::invalid_input("verification.summary is required"));
    }
    Ok(GoalCompletionVerification {
        status: "passed".to_string(),
        check: verification.check.trim().to_string(),
        summary: verification.summary.trim().to_string(),
    })
}

fn json_result(snapshot: &GoalSnapshot) -> Result<ToolResult, ToolError> {
    ToolResult::json(snapshot).map_err(|err| ToolError::execution_failed(err.to_string()))
}

pub struct GoalEnqueueTool {
    goal_queue: SharedGoalQueue,
}

impl GoalEnqueueTool {
    #[must_use]
    pub fn new(goal_queue: SharedGoalQueue) -> Self {
        Self { goal_queue }
    }
}

#[async_trait]
impl ToolSpec for GoalEnqueueTool {
    fn name(&self) -> &'static str {
        "goal_enqueue"
    }

    fn description(&self) -> &'static str {
        "Enqueue a persistent objective into the goal queue. Multiple goals can be queued without overwriting each other. The highest-priority ready goal (dependencies satisfied) becomes active automatically. Use only when the user explicitly asks to pursue a persistent objective."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "objective": {
                    "type": "string",
                    "description": "The full objective to pursue. Keep the complete user goal, not a shortened one-turn version."
                },
                "token_budget": {
                    "type": "integer",
                    "minimum": 0,
                    "description": "Optional soft token budget for this goal."
                },
                "priority": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 255,
                    "description": "Optional调度优先级 (0-255，越大越先执行)。默认 0。"
                },
                "blocked_by": {
                    "type": "array",
                    "items": { "type": "integer" },
                    "description": "Optional goal ids that must complete before this one starts. Self-references, unknown ids, and cycle-closing edges are ignored."
                }
            },
            "required": ["objective"],
            "additionalProperties": false
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        Vec::new()
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    async fn execute(&self, input: Value, _context: &ToolContext) -> Result<ToolResult, ToolError> {
        let objective = required_str(&input, "objective")?.trim().to_string();
        if objective.is_empty() {
            return Err(ToolError::invalid_input("objective cannot be empty"));
        }
        let token_budget = parse_token_budget(&input)?;
        let priority = input
            .get("priority")
            .and_then(Value::as_u64)
            .and_then(|v| u8::try_from(v).ok())
            .unwrap_or(0);
        let blocked_by: Vec<u32> = input
            .get("blocked_by")
            .and_then(Value::as_array)
            .map(|deps| {
                deps.iter()
                    .filter_map(|d| d.as_u64().and_then(|d| u32::try_from(d).ok()))
                    .collect()
            })
            .unwrap_or_default();
        let id = {
            let mut queue = lock_goal_queue(&self.goal_queue)?;
            queue.enqueue(objective, token_budget, priority, blocked_by)
        };
        let snapshot = {
            let queue = lock_goal_queue(&self.goal_queue)?;
            queue.snapshot_of(id)
        };
        match snapshot {
            Some(snap) => json_result(&snap),
            None => Err(ToolError::execution_failed("enqueued goal vanished")),
        }
    }
}

pub struct GoalGetTool {
    goal_queue: SharedGoalQueue,
}

impl GoalGetTool {
    #[must_use]
    pub fn new(goal_queue: SharedGoalQueue) -> Self {
        Self { goal_queue }
    }
}

#[async_trait]
impl ToolSpec for GoalGetTool {
    fn name(&self) -> &'static str {
        "goal_get"
    }

    fn description(&self) -> &'static str {
        "Inspect a goal's runtime state (objective, status, token budget, elapsed time, evidence, blocker). Returns the active goal by default; pass `id` to inspect a specific queued goal."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "integer",
                    "description": "Optional goal id to inspect. Omit to return the active goal."
                }
            },
            "additionalProperties": false
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    fn supports_parallel(&self) -> bool {
        true
    }

    async fn execute(&self, input: Value, _context: &ToolContext) -> Result<ToolResult, ToolError> {
        let id = input
            .get("id")
            .and_then(Value::as_u64)
            .and_then(|v| u32::try_from(v).ok());
        let snapshot = {
            let queue = lock_goal_queue(&self.goal_queue)?;
            match id {
                Some(id) => queue.snapshot_of(id),
                None => queue.active_snapshot(),
            }
        };
        match snapshot {
            Some(snap) => json_result(&snap),
            None => Err(ToolError::invalid_input(
                "no active goal and no matching id; enqueue one with goal_enqueue",
            )),
        }
    }
}

pub struct GoalListTool {
    goal_queue: SharedGoalQueue,
}

impl GoalListTool {
    #[must_use]
    pub fn new(goal_queue: SharedGoalQueue) -> Self {
        Self { goal_queue }
    }
}

#[async_trait]
impl ToolSpec for GoalListTool {
    fn name(&self) -> &'static str {
        "goal_list"
    }

    fn description(&self) -> &'static str {
        "List the full goal queue: every goal's id, priority, queue status, objective, token budget usage, dependencies, and current readiness. Use this to see what is queued, active, paused, or done."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    fn supports_parallel(&self) -> bool {
        true
    }

    async fn execute(
        &self,
        _input: Value,
        _context: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let snapshot = {
            let queue = lock_goal_queue(&self.goal_queue)?;
            queue.list_snapshot()
        };
        ToolResult::json(&snapshot).map_err(|err| ToolError::execution_failed(err.to_string()))
    }
}

pub struct GoalUpdateTool {
    goal_queue: SharedGoalQueue,
}

impl GoalUpdateTool {
    #[must_use]
    pub fn new(goal_queue: SharedGoalQueue) -> Self {
        Self { goal_queue }
    }

    /// Independent completion judge (#T-Q4).
    ///
    /// The model submits a self-reported `verification` when calling
    /// `goal_update` with `status: "complete"`. This is an *independent*
    /// second opinion: it scores the supplied `evidence` (and the verifier
    /// `check` name) through the standalone [`reviewer`] module rather than
    /// trusting the self-report. The judge only rejects when the evidence is
    /// directly contradicted (e.g. the evidence still shows a failure while
    /// the model claims success) — a `Rejected` verdict forces the model to
    /// supply stronger, reproducible proof before the goal can close.
    ///
    /// Returns `Ok(())` when the independent judge agrees (or cannot
    /// contradict) the completion, and `Err(reason)` when it must be rejected.
    fn independent_judge(
        &self,
        objective: &str,
        evidence: &str,
        check: &str,
    ) -> Result<(), String> {
        let evidence_lower = evidence.to_lowercase();

        // Contradiction: the model claims success but the evidence still shows
        // a failure AND contains no corroborating pass signal. A log that
        // mentions an error while also reporting a passing check is *not* a
        // contradiction (e.g. "fixed the error; tests pass").
        let has_pass_signal = evidence_lower.contains("pass")
            || evidence_lower.contains("ok")
            || evidence_lower.contains("success")
            || evidence_lower.trim().is_empty();
        let has_failure_signal = evidence_lower.contains("error")
            || evidence_lower.contains("fail")
            || evidence_lower.contains("panic")
            || evidence_lower.contains("✗");
        let contradicted = has_failure_signal && !has_pass_signal;

        // Evidence strength: a verifier/check that passed (e.g. test run,
        // build, lint) is strong; a reproduction path without a pass keyword is
        // medium; bare assertion is weak.
        let has_verifier_pass = evidence_lower.contains("test result: ok")
            || evidence_lower.contains("passed")
            || evidence_lower.contains("0 failed")
            || evidence_lower.contains("build succeeded")
            || check.to_lowercase().contains("verif")
            || check.to_lowercase().contains("test");

        let strength = if has_verifier_pass {
            EvidenceStrength::Strong
        } else if !evidence.trim().is_empty() {
            EvidenceStrength::Medium
        } else {
            EvidenceStrength::Weak
        };

        // Reproducible steps: evidence references a concrete command, path, or
        // script (heuristic: contains a path-like or shell token).
        let has_repro_steps = evidence.contains('/')
            || evidence.contains('\\')
            || evidence_lower.contains("cargo ")
            || evidence_lower.contains("npm ")
            || evidence_lower.contains("pytest")
            || evidence_lower.contains("run ")
            || evidence.contains('`');

        let claim = ClaimForReview {
            title: objective.to_string(),
            strength,
            has_repro_steps,
            contradicted,
        };

        match reviewer::review(&claim) {
            ReviewVerdict::Rejected => Err(format!(
                "evidence contradicts the claimed completion (objective: {objective}; evidence: {evidence})"
            )),
            // Accepted / Weak both let the goal close; the model's verifier
            // receipt is accepted as the authority unless directly contradicted.
            ReviewVerdict::Accepted | ReviewVerdict::Weak => Ok(()),
        }
    }
}

#[async_trait]
impl ToolSpec for GoalUpdateTool {
    fn name(&self) -> &'static str {
        "goal_update"
    }

    fn description(&self) -> &'static str {
        "Update the ACTIVE goal's completion gate. Only mark complete when the objective has verified evidence; mark blocked only after a real blocker prevents progress. Completing or blocking the active goal automatically promotes the next ready queued goal."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "status": {
                    "type": "string",
                    "enum": ["complete", "blocked"],
                    "description": "complete only when fully satisfied; blocked when meaningful progress cannot continue."
                },
                "evidence": {
                    "type": "string",
                    "description": "Required when status is complete. Briefly cite the proof that the goal is done."
                },
                "verification": {
                    "type": "object",
                    "description": "Required when status is complete. A verifier-as-judge receipt from a concrete check, such as run_verifiers or an equivalent project-specific gate.",
                    "properties": {
                        "status": {
                            "type": "string",
                            "enum": ["passed"],
                            "description": "Must be passed before the goal can be marked complete."
                        },
                        "check": {
                            "type": "string",
                            "description": "The verifier/check that passed."
                        },
                        "summary": {
                            "type": "string",
                            "description": "Brief result summary from the verifier/check."
                        }
                    },
                    "required": ["status", "check", "summary"],
                    "additionalProperties": false
                },
                "blocker": {
                    "type": "string",
                    "description": "Required when status is blocked. Explain the condition preventing progress."
                }
            },
            "required": ["status"],
            "additionalProperties": false
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        Vec::new()
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    async fn execute(&self, input: Value, _context: &ToolContext) -> Result<ToolResult, ToolError> {
        let status = required_str(&input, "status")?.trim().to_ascii_lowercase();
        let active_id = {
            let queue = lock_goal_queue(&self.goal_queue)?;
            queue.active_id()
        };
        let Some(active_id) = active_id else {
            return Err(ToolError::invalid_input(
                "no active goal to update; enqueue one with goal_enqueue",
            ));
        };
        let result: std::result::Result<(), ToolError> = {
            let mut queue = lock_goal_queue(&self.goal_queue)?;
            match status.as_str() {
                "complete" => {
                    let evidence = input
                        .get("evidence")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .unwrap_or_default()
                        .to_string();
                    if evidence.is_empty() {
                        return Err(ToolError::invalid_input(
                            "evidence is required when status is complete",
                        ));
                    }
                    let verification = parse_completion_verification(&input)?;
                    // Independent LLM-as-judge gate (#T-Q4): do not take the
                    // model's self-reported completion at face value. Run the
                    // evidence through the standalone reviewer so a goal can
                    // only be marked complete when an independent check agrees
                    // the objective is actually met.
                    let objective = {
                        let q = lock_goal_queue(&self.goal_queue)?;
                        q.active_snapshot()
                            .and_then(|s| s.objective.clone())
                            .unwrap_or_default()
                    };
                    if let Err(reason) =
                        self.independent_judge(&objective, &evidence, &verification.check)
                    {
                        return Err(ToolError::invalid_input(format!(
                            "independent judge rejected goal completion: {reason}. Provide stronger, reproducible evidence (e.g. a passing verifier/check) before marking complete."
                        )));
                    }
                    queue
                        .mark_complete(active_id, evidence, verification)
                        .map_err(ToolError::invalid_input)?;
                    Ok(())
                }
                "blocked" => {
                    let blocker = input
                        .get("blocker")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .unwrap_or_default()
                        .to_string();
                    if blocker.is_empty() {
                        return Err(ToolError::invalid_input(
                            "blocker is required when status is blocked",
                        ));
                    }
                    queue
                        .mark_blocked(active_id, blocker)
                        .map_err(ToolError::invalid_input)?;
                    Ok(())
                }
                other => {
                    return Err(ToolError::invalid_input(format!(
                        "unsupported goal status '{other}'; goal_update can only mark complete or blocked"
                    )));
                }
            }
        };
        result?;
        let snapshot = {
            let queue = lock_goal_queue(&self.goal_queue)?;
            queue.list_snapshot()
        };
        ToolResult::json(&snapshot).map_err(|err| ToolError::execution_failed(err.to_string()))
    }
}

/// Helper: resolve an optional `id` arg or fall back to the active goal id.
fn resolve_target_id(queue: &GoalQueue, input: &Value) -> Result<u32, ToolError> {
    if let Some(id) = input
        .get("id")
        .and_then(Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
    {
        return Ok(id);
    }
    queue
        .active_id()
        .ok_or_else(|| ToolError::invalid_input("no active goal and no `id` provided"))
}

pub struct GoalPauseTool {
    goal_queue: SharedGoalQueue,
}

impl GoalPauseTool {
    #[must_use]
    pub fn new(goal_queue: SharedGoalQueue) -> Self {
        Self { goal_queue }
    }
}

#[async_trait]
impl ToolSpec for GoalPauseTool {
    fn name(&self) -> &'static str {
        "goal_pause"
    }

    fn description(&self) -> &'static str {
        "Pause a goal (default: the active one). A paused goal stops occupying the execution slot and the next ready queued goal is promoted. Pass `id` to pause a specific goal."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "integer",
                    "description": "Optional goal id to pause. Omit to pause the active goal."
                }
            },
            "additionalProperties": false
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        Vec::new()
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    async fn execute(&self, input: Value, _context: &ToolContext) -> Result<ToolResult, ToolError> {
        let id = {
            let queue = lock_goal_queue(&self.goal_queue)?;
            resolve_target_id(&queue, &input)?
        };
        let result = {
            let mut queue = lock_goal_queue(&self.goal_queue)?;
            queue.pause(id).map_err(ToolError::invalid_input)
        };
        let snapshot = {
            let queue = lock_goal_queue(&self.goal_queue)?;
            queue.list_snapshot()
        };
        result?;
        ToolResult::json(&snapshot).map_err(|err| ToolError::execution_failed(err.to_string()))
    }
}

pub struct GoalResumeTool {
    goal_queue: SharedGoalQueue,
}

impl GoalResumeTool {
    #[must_use]
    pub fn new(goal_queue: SharedGoalQueue) -> Self {
        Self { goal_queue }
    }
}

#[async_trait]
impl ToolSpec for GoalResumeTool {
    fn name(&self) -> &'static str {
        "goal_resume"
    }

    fn description(&self) -> &'static str {
        "Resume a paused goal by id, returning it to the queue so it can be promoted when a slot opens."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "integer",
                    "description": "Goal id to resume (required; only paused goals can resume)."
                }
            },
            "required": ["id"],
            "additionalProperties": false
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        Vec::new()
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    async fn execute(&self, input: Value, _context: &ToolContext) -> Result<ToolResult, ToolError> {
        let id = input
            .get("id")
            .and_then(Value::as_u64)
            .and_then(|v| u32::try_from(v).ok())
            .ok_or_else(|| ToolError::invalid_input("`id` (a paused goal) is required"))?;
        let result = {
            let mut queue = lock_goal_queue(&self.goal_queue)?;
            queue.resume(id).map_err(ToolError::invalid_input)
        };
        let snapshot = {
            let queue = lock_goal_queue(&self.goal_queue)?;
            queue.list_snapshot()
        };
        result?;
        ToolResult::json(&snapshot).map_err(|err| ToolError::execution_failed(err.to_string()))
    }
}

pub struct GoalCancelTool {
    goal_queue: SharedGoalQueue,
}

impl GoalCancelTool {
    #[must_use]
    pub fn new(goal_queue: SharedGoalQueue) -> Self {
        Self { goal_queue }
    }
}

#[async_trait]
impl ToolSpec for GoalCancelTool {
    fn name(&self) -> &'static str {
        "goal_cancel"
    }

    fn description(&self) -> &'static str {
        "Cancel a goal by id, removing it from scheduling. The next ready queued goal is promoted. Cancelled goals remain visible in goal_list as done."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "integer",
                    "description": "Goal id to cancel (required)."
                }
            },
            "required": ["id"],
            "additionalProperties": false
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        Vec::new()
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    async fn execute(&self, input: Value, _context: &ToolContext) -> Result<ToolResult, ToolError> {
        let id = input
            .get("id")
            .and_then(Value::as_u64)
            .and_then(|v| u32::try_from(v).ok())
            .ok_or_else(|| ToolError::invalid_input("`id` (a goal to cancel) is required"))?;
        let result = {
            let mut queue = lock_goal_queue(&self.goal_queue)?;
            queue.cancel(id).map_err(ToolError::invalid_input)
        };
        let snapshot = {
            let queue = lock_goal_queue(&self.goal_queue)?;
            queue.list_snapshot()
        };
        result?;
        ToolResult::json(&snapshot).map_err(|err| ToolError::execution_failed(err.to_string()))
    }
}

pub struct GoalPromoteTool {
    goal_queue: SharedGoalQueue,
}

impl GoalPromoteTool {
    #[must_use]
    pub fn new(goal_queue: SharedGoalQueue) -> Self {
        Self { goal_queue }
    }
}

#[async_trait]
impl ToolSpec for GoalPromoteTool {
    fn name(&self) -> &'static str {
        "goal_promote"
    }

    fn description(&self) -> &'static str {
        "Manually promote a queued goal to active by id. Only succeeds when no goal is currently active and the target's dependencies are satisfied."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "integer",
                    "description": "Goal id to promote to active (required)."
                }
            },
            "required": ["id"],
            "additionalProperties": false
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        Vec::new()
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    async fn execute(&self, input: Value, _context: &ToolContext) -> Result<ToolResult, ToolError> {
        let id = input
            .get("id")
            .and_then(Value::as_u64)
            .and_then(|v| u32::try_from(v).ok())
            .ok_or_else(|| ToolError::invalid_input("`id` (a queued goal) is required"))?;
        let result = {
            let mut queue = lock_goal_queue(&self.goal_queue)?;
            queue.promote(id).map_err(ToolError::invalid_input)
        };
        let snapshot = {
            let queue = lock_goal_queue(&self.goal_queue)?;
            queue.list_snapshot()
        };
        result?;
        ToolResult::json(&snapshot).map_err(|err| ToolError::execution_failed(err.to_string()))
    }
}
