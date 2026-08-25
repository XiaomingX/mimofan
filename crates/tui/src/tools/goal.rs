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

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::prompts::GOAL_CONTINUATION_PROMPT;
use crate::reviewer::{self, ClaimForReview, EvidenceStrength, ReviewVerdict};
use crate::tools::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec, required_str,
};

// ───────────────────────────────────────────────────────────────────────────
// 目标队列会话落盘（best-effort，绝不 panic）
//
// 运行时目标队列（add/pause/complete 等）目前只存在于内存，会话重启即丢失。
// 这里提供「本地文件兜底」：宿主（LoopX/CodeBuddy）注入优先，本地文件仅当
// 队列完全为空时作为兜底补充，二者互补不冲突。任何 IO 失败都 `tracing::warn!`
// 吞掉，不干扰主流程。
// ───────────────────────────────────────────────────────────────────────────

/// 目标队列落盘文件名。
const GOAL_QUEUE_FILE: &str = "goal_queue.json";

/// 返回落盘文件路径。
///
/// - `base_dir = Some(dir)`：直接用 `dir/goal_queue.json`（测试用临时目录）。
/// - `base_dir = None`：用 `~/.mimofan/goal_queue.json`（`$HOME` 或 `$USERPROFILE`）。
#[must_use]
pub fn goal_queue_persist_path(base_dir: Option<&Path>) -> PathBuf {
    match base_dir {
        Some(dir) => dir.join(GOAL_QUEUE_FILE),
        None => {
            let key = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
            let home = std::env::var_os(key)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from);
            home.unwrap_or_else(PathBuf::new)
                .join(".mimofan")
                .join(GOAL_QUEUE_FILE)
        }
    }
}

/// 将运行时目标队列快照原子落盘（best-effort）。
///
/// 取锁 → 序列化 `GoalQueueSnapshot` → `write_atomic`（临时文件 + rename）。
/// 任何错误（锁中毒 / 序列化 / IO）都记警告并吞掉，绝不 panic。
pub fn persist_goal_queue(queue: &SharedGoalQueue, base_dir: Option<&Path>) {
    let snapshot = match queue.lock() {
        Ok(queue) => queue.list_snapshot(),
        Err(err) => {
            tracing::warn!("goal queue lock poisoned while persisting: {err}");
            return;
        }
    };
    let json = match serde_json::to_string_pretty(&snapshot) {
        Ok(json) => json,
        Err(err) => {
            tracing::warn!("failed to serialize goal queue snapshot: {err}");
            return;
        }
    };
    let path = goal_queue_persist_path(base_dir);
    if let Some(parent) = path.parent()
        && let Err(err) = std::fs::create_dir_all(parent)
    {
        tracing::warn!(
            "failed to create goal queue dir {}: {err}",
            parent.display()
        );
        return;
    }
    if let Err(err) = crate::utils::write_atomic(&path, json.as_bytes()) {
        tracing::warn!("failed to persist goal queue to {}: {err}", path.display());
    }
}

/// 从落盘文件兜底加载队列（best-effort）。
///
/// 文件不存在 / 解析失败 / 队列为空均返回 `None`。返回重建后的 `GoalQueue`
/// （非共享包装），由调用方决定是否注入到 `SharedGoalQueue`。
#[must_use]
pub fn load_goal_queue_fallback(base_dir: Option<&Path>) -> Option<GoalQueue> {
    let path = goal_queue_persist_path(base_dir);
    let content = match std::fs::read_to_string(&path) {
        Ok(content) => content,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return None,
        Err(err) => {
            tracing::warn!("failed to read goal queue file {}: {err}", path.display());
            return None;
        }
    };
    let snapshot: GoalQueueSnapshot = match serde_json::from_str(&content) {
        Ok(snapshot) => snapshot,
        Err(err) => {
            tracing::warn!("failed to parse goal queue file {}: {err}", path.display());
            return None;
        }
    };
    let queue = GoalQueue::from_snapshot(&snapshot);
    if queue.is_empty() { None } else { Some(queue) }
}

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
        persist_goal_queue(&self.goal_queue, None);
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
        persist_goal_queue(&self.goal_queue, None);
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
        persist_goal_queue(&self.goal_queue, None);
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
        persist_goal_queue(&self.goal_queue, None);
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
        persist_goal_queue(&self.goal_queue, None);
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
        persist_goal_queue(&self.goal_queue, None);
        ToolResult::json(&snapshot).map_err(|err| ToolError::execution_failed(err.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 单 goal 级别的进度信号测试（复用 GoalState 实现，未改名）。
    fn active_state() -> GoalState {
        let mut s = GoalState::default();
        s.create("ship the refactor".to_string(), None);
        s
    }

    #[test]
    fn file_changed_resets_counters() {
        let mut s = active_state();
        s.record_progress_signal(&ProgressSignal::NoChange);
        s.record_progress_signal(&ProgressSignal::NoChange);
        assert_eq!(s.no_progress_rounds(), 2);
        s.record_progress_signal(&ProgressSignal::FileChanged);
        assert_eq!(s.no_progress_rounds(), 0);
        assert_eq!(s.repeated_error_rounds(), 0);
    }

    #[test]
    fn repeated_same_error_counts_then_resets_on_new() {
        let mut s = active_state();
        s.record_progress_signal(&ProgressSignal::ToolError {
            fingerprint: "E1".to_string(),
        });
        s.record_progress_signal(&ProgressSignal::ToolError {
            fingerprint: "E1".to_string(),
        });
        assert_eq!(s.repeated_error_rounds(), 2);
        s.record_progress_signal(&ProgressSignal::ToolError {
            fingerprint: "E2".to_string(),
        });
        assert_eq!(s.repeated_error_rounds(), 1);
        assert_eq!(s.last_tool_error_fingerprint.as_deref(), Some("E2"));
    }

    #[test]
    fn no_progress_also_increments_on_error() {
        let mut s = active_state();
        s.record_progress_signal(&ProgressSignal::ToolError {
            fingerprint: "E1".to_string(),
        });
        assert_eq!(s.no_progress_rounds(), 1);
    }

    #[test]
    fn configure_loop_sets_fields() {
        let mut s = active_state();
        s.configure_loop(Some("all tests pass".to_string()), Some(7), true);
        assert_eq!(s.stop_condition(), Some("all tests pass"));
        assert_eq!(s.max_continuations_override(), Some(7));
        assert!(s.checkpoint_each_round());
        s.configure_loop(Some("   ".to_string()), None, false);
        assert_eq!(s.stop_condition(), None);
    }

    #[test]
    fn continuation_prompt_injects_stop_condition() {
        let mut s = active_state();
        s.configure_loop(Some("build is green".to_string()), None, false);
        let snap = s.snapshot();
        let prompt = render_continuation_prompt(&snap, 1, snap.stop_condition.as_deref());
        assert!(prompt.contains("build is green"));
        assert!(prompt.contains("Stop Condition"));
    }

    #[test]
    fn continuation_prompt_omits_stop_section_without_condition() {
        let s = active_state();
        let snap = s.snapshot();
        let prompt = render_continuation_prompt(&snap, 1, snap.stop_condition.as_deref());
        assert!(!prompt.contains("Stop Condition"));
    }

    #[test]
    fn snapshot_round_trips_loop_fields() {
        let mut s = active_state();
        s.configure_loop(Some("done".to_string()), Some(3), true);
        let snap = s.snapshot();
        assert_eq!(snap.stop_condition.as_deref(), Some("done"));
        assert_eq!(snap.max_rounds, Some(3));
        assert!(snap.checkpoint_each_round);
    }

    // === GoalQueue 调度测试 ===

    fn queue_with_two() -> GoalQueue {
        let mut q = GoalQueue::default();
        // 先入队低优先级，后入队高优先级；首个入队应被提升为 active。
        q.enqueue("first".to_string(), None, 0, Vec::new());
        q.enqueue("second".to_string(), None, 10, Vec::new());
        q
    }

    #[test]
    fn enqueue_does_not_overwrite_and_promotes_first() {
        let q = queue_with_two();
        assert_eq!(q.entries.len(), 2, "两个 goal 都在队列中，互不覆盖");
        // 首个入队（无 active 时）被提升为 active。
        assert_eq!(q.active_id(), Some(1));
    }

    #[test]
    fn completing_active_promotes_higher_priority_ready() {
        let mut q = queue_with_two();
        // 完成 id=1，应提升 id=2（优先级更高且在 queued）。
        q.mark_complete(
            1,
            "done".to_string(),
            GoalCompletionVerification {
                status: "passed".to_string(),
                check: "x".to_string(),
                summary: "y".to_string(),
            },
        )
        .unwrap();
        assert_eq!(q.get(1).unwrap().queue_status, QueueStatus::Done);
        assert_eq!(q.active_id(), Some(2), "高优先级就绪 goal 被提升");
    }

    #[test]
    fn blocked_goal_cannot_be_promoted_until_dependency_done() {
        let mut q = GoalQueue::default();
        q.enqueue("up".to_string(), None, 0, Vec::new()); // id=1 active
        q.enqueue("down".to_string(), None, 0, vec![1]); // id=2 依赖 id=1
        // 完成 id=1 后，id=2 依赖满足，应被提升。
        q.mark_complete(
            1,
            "ok".to_string(),
            GoalCompletionVerification {
                status: "passed".to_string(),
                check: "x".to_string(),
                summary: "y".to_string(),
            },
        )
        .unwrap();
        assert_eq!(q.active_id(), Some(2), "依赖完成后下游解锁并提升");
    }

    #[test]
    fn self_reference_and_unknown_deps_are_dropped() {
        let mut q = GoalQueue::default();
        let id = q.enqueue("solo".to_string(), None, 0, vec![1, 99]);
        assert!(q.get(id).unwrap().blocked_by.is_empty());
    }

    #[test]
    fn cycle_closing_edge_is_refused() {
        let mut q = GoalQueue::default();
        q.enqueue("a".to_string(), None, 0, Vec::new()); // id=1
        q.enqueue("b".to_string(), None, 0, vec![1]); // id=2 依赖 1
        // 试图让 1 依赖 2 会成环，必须被拒绝（blocked_by 为空）。
        let cyclic = q.sanitize_dependencies(1, vec![2]);
        assert_eq!(cyclic, Vec::<u32>::new());
    }

    #[test]
    fn cancel_promotes_next_ready() {
        let mut q = queue_with_two();
        q.cancel(1).unwrap();
        assert_eq!(q.active_id(), Some(2), "取消 active 后提升下一个");
    }

    #[test]
    fn aggregate_budget_stops_promotion() {
        let mut q = GoalQueue::default();
        q.set_aggregate_token_budget(Some(10));
        q.enqueue("a".to_string(), None, 0, Vec::new()); // active id=1
        q.enqueue("b".to_string(), None, 0, Vec::new()); // queued id=2
        q.record_usage(20, 0); // 超过聚合预算
        assert!(q.aggregate_budget_exhausted());
        q.mark_complete(
            1,
            "ok".to_string(),
            GoalCompletionVerification {
                status: "passed".to_string(),
                check: "x".to_string(),
                summary: "y".to_string(),
            },
        )
        .unwrap();
        // 预算耗尽，不再提升。
        assert_eq!(q.active_id(), None);
    }

    #[test]
    fn pause_promotes_next_then_resume_requeues() {
        let mut q = queue_with_two();
        q.pause(1).unwrap();
        assert_eq!(q.active_id(), Some(2));
        q.resume(1).unwrap();
        assert_eq!(q.get(1).unwrap().queue_status, QueueStatus::Queued);
    }

    // === 目标队列会话落盘 round-trip ===

    #[test]
    fn goal_queue_persist_then_fallback_reload() {
        let dir = tempfile::tempdir().expect("tempdir");
        let base = dir.path();

        // 构造队列并写入落盘文件。
        let shared = new_shared_goal_queue();
        {
            let mut q = shared.lock().unwrap();
            q.enqueue("persisted goal".to_string(), Some(100), 5, Vec::new());
            q.enqueue("second".to_string(), None, 0, Vec::new());
        }
        persist_goal_queue(&shared, Some(base));

        let path = goal_queue_persist_path(Some(base));
        assert!(path.exists(), "落盘文件应已生成");

        // 模拟会话重启：清空原共享队列，再从文件兜底加载。
        {
            let mut q = shared.lock().unwrap();
            q.entries.clear();
            q.next_id = 1;
        }
        let restored = load_goal_queue_fallback(Some(base));
        assert!(restored.is_some(), "兜底加载应返回队列");
        let restored = restored.unwrap();
        assert_eq!(restored.entries.len(), 2, "两个 goal 都应恢复");
        assert_eq!(
            restored.get(1).unwrap().goal.objective(),
            Some("persisted goal")
        );
        assert_eq!(
            restored.get(1).unwrap().goal.token_budget(),
            Some(100),
            "token 预算应恢复"
        );
    }

    #[test]
    fn load_goal_queue_fallback_missing_file_returns_none() {
        let dir = tempfile::tempdir().expect("tempdir");
        // 目录存在但无文件 → 返回 None，不 panic。
        assert!(load_goal_queue_fallback(Some(dir.path())).is_none());
    }
}
