//! 纯逻辑的 goal 目标管理状态机。
//!
//! 本 crate 从 `crates/tui/src/tools/goal.rs` 下沉而来，承载 goal 的
//! 运行时状态（`GoalState`）、多目标调度队列（`GoalQueue`）以及相关的
//! 可序列化快照类型。**不依赖 TUI**：所有与 UI / prompts / reviewer 耦合的
//! 部分（工具实现、`independent_judge`、续跑 prompt 的 base 文案）留在 tui 层，
//! 通过参数注入或薄包装复用本 crate。
//!
//! 设计约束（与 `ARCHITECTURE_STABILITY.md` §8.3 对齐）：
//! 目标状态的读写全部发生在同步代码块内，且 `std::sync::Mutex` 守卫**绝不**
//! 跨 `.await` 持有。

use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::{Deserialize, Serialize};

// === 类型与常量 ===

/// Maximum number of automatic goal-continuation prompt injections in one
/// engine turn. This is intra-turn granularity only — it prevents a stuck spin
/// within a single turn from making no progress. The cross-turn loop has **no
/// cap**: a goal runs until complete/blocked/paused, or an optional budget is
/// exhausted. See `goal_loop::decide_continuation`.
pub const MAX_GOAL_CONTINUATIONS_PER_TURN: u32 = 3;

/// Runtime status for a goal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoalStatus {
    Active,
    Paused,
    Complete,
    Blocked,
}

/// `/loop`-specific configuration carried from the `/loop` command into the
/// engine's `SharedGoalQueue`. `None` fields mean "not a loop / use defaults".
/// This is the wire type between the UI (`AppAction::SetGoalStatus`) and the
/// engine (`Op::SetGoalStatus`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LoopConfig {
    /// User-defined natural-language stop condition (Claude Code `/loop` parity).
    pub stop_condition: Option<String>,
    /// Explicit round cap, overriding `DEFAULT_MAX_CONTINUATIONS`.
    pub max_rounds: Option<u32>,
    /// Snapshot the workspace before each continuation round for `/rewind`.
    pub checkpoint_each_round: bool,
}

impl GoalStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Paused => "paused",
            Self::Complete => "complete",
            Self::Blocked => "blocked",
        }
    }

    /// 从落盘快照里的字符串还原（默认 `Active`，与 `GoalState::default` 对齐）。
    #[must_use]
    pub fn parse_str(value: &str) -> Self {
        match value {
            "paused" => Self::Paused,
            "complete" => Self::Complete,
            "blocked" => Self::Blocked,
            _ => Self::Active,
        }
    }
}

/// Signal reported by the engine at the end of each turn, used to drive the
/// anti-drift guardrails (no-progress / repeated-error circuit breakers).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgressSignal {
    /// At least one file changed during the turn — forward progress.
    FileChanged,
    /// No file changes this turn (and no tool error worth counting).
    NoChange,
    /// A tool errored; `fingerprint` is a stable hash-ish key (e.g. the
    /// error message trimmed to a bounded length) used to detect repeats.
    ToolError { fingerprint: String },
}

/// Default circuit-breaker thresholds (kept loose so normal long tasks are
/// not interrupted). Tunable via env `MIMOFAN_GOAL_NO_PROGRESS_ROUNDS` /
/// `MIMOFAN_GOAL_REPEATED_ERROR_ROUNDS`.
pub const DEFAULT_NO_PROGRESS_ROUNDS: u32 = 8;
pub const DEFAULT_REPEATED_ERROR_ROUNDS: u32 = 5;

/// Session-local goal state. `Instant` stays runtime-only; snapshots expose
/// elapsed seconds so tool output remains serializable and stable.
#[derive(Debug, Clone, Default)]
pub struct GoalState {
    objective: Option<String>,
    token_budget: Option<u32>,
    status: Option<GoalStatus>,
    tokens_used: u64,
    time_used_seconds: u64,
    continuation_count: u32,
    started_at: Option<Instant>,
    finished_at: Option<Instant>,
    evidence: Option<String>,
    blocker: Option<String>,
    completion_verification: Option<GoalCompletionVerification>,
    /// Human-readable progress checklist (done / todo lines) shown in the
    /// goal contract injected into the system prompt.
    progress_checklist: Option<String>,
    /// Consecutive turns with no file changes — feeds the no-progress breaker.
    no_progress_rounds: u32,
    /// Last tool-error fingerprint; `None` resets the repeated-error counter.
    pub last_tool_error_fingerprint: Option<String>,
    /// Consecutive turns ending on the *same* tool-error fingerprint.
    repeated_error_rounds: u32,
    /// Optional wall-clock budget in seconds (wired into `decide_continuation`).
    time_budget_seconds: Option<u64>,
    /// User-defined natural-language stop condition for `/loop` (Claude Code
    /// `/loop` parity). The model self-judges it each round; `None` for a plain
    /// `/goal`. Injected into the continuation prompt by `render_continuation_prompt`.
    stop_condition: Option<String>,
    /// Explicit round cap for `/loop`, overriding the default safety cap in
    /// `decide_continuation`. `None` falls back to `DEFAULT_MAX_CONTINUATIONS`.
    max_rounds: Option<u32>,
    /// When true, each loop round snapshots the workspace via side-git so the
    /// user can `/rewind` to a specific round.
    checkpoint_each_round: bool,
    /// Loop config staged by `/loop` before the model has created the objective
    /// via `goal_enqueue`. Applied (and cleared) once the objective exists, so a
    /// `SetGoalStatus{loop_config}` arriving before the first turn is not lost.
    pub(crate) pending_loop_config: Option<LoopConfig>,
}

impl GoalState {
    #[must_use]
    pub fn objective(&self) -> Option<&str> {
        self.objective.as_deref()
    }

    /// The completion verification attached to the goal, if any.
    #[must_use]
    pub fn completion_verification(&self) -> Option<&GoalCompletionVerification> {
        self.completion_verification.as_ref()
    }

    /// The progress checklist, if set.
    #[must_use]
    pub fn progress_checklist(&self) -> Option<&str> {
        self.progress_checklist.as_deref()
    }

    #[must_use]
    pub fn token_budget(&self) -> Option<u32> {
        self.token_budget
    }

    #[must_use]
    pub fn is_active(&self) -> bool {
        self.objective.is_some() && self.status == Some(GoalStatus::Active)
    }

    pub fn sync_from_host_status(
        &mut self,
        objective: Option<&str>,
        token_budget: Option<u32>,
        status: GoalStatus,
    ) {
        let objective = objective.map(str::trim).filter(|value| !value.is_empty());
        match objective {
            Some(objective) => {
                let changed = self.objective.as_deref() != Some(objective);
                let status_changed = self.status != Some(status);
                if changed {
                    self.objective = Some(objective.to_string());
                    self.token_budget = token_budget;
                    self.tokens_used = 0;
                    self.time_used_seconds = 0;
                    self.continuation_count = 0;
                    self.started_at = Some(Instant::now());
                    self.evidence = None;
                    self.blocker = None;
                    self.completion_verification = None;
                    self.progress_checklist = None;
                    self.no_progress_rounds = 0;
                    self.last_tool_error_fingerprint = None;
                    self.repeated_error_rounds = 0;
                    self.time_budget_seconds = None;
                    // `/goal` carries no loop-specific fields; reset them so a
                    // previous `/loop` does not leak into a subsequent `/goal`.
                    self.stop_condition = None;
                    self.max_rounds = None;
                    self.checkpoint_each_round = false;
                    // Drop any staged `/loop` config — a fresh `/goal` objective
                    // is not a loop.
                    self.pending_loop_config = None;
                } else if self.token_budget != token_budget {
                    self.token_budget = token_budget;
                }

                if changed || status_changed || self.status.is_none() {
                    self.status = Some(status);
                    self.finished_at = if status == GoalStatus::Active {
                        None
                    } else {
                        Some(Instant::now())
                    };
                }
            }
            None => self.clear(),
        }
    }

    pub fn create(&mut self, objective: String, token_budget: Option<u32>) {
        self.objective = Some(objective);
        self.token_budget = token_budget;
        self.status = Some(GoalStatus::Active);
        self.tokens_used = 0;
        self.time_used_seconds = 0;
        self.continuation_count = 0;
        self.started_at = Some(Instant::now());
        self.finished_at = None;
        self.evidence = None;
        self.blocker = None;
        self.completion_verification = None;
        self.progress_checklist = None;
        self.no_progress_rounds = 0;
        self.last_tool_error_fingerprint = None;
        self.repeated_error_rounds = 0;
        self.time_budget_seconds = None;
        self.stop_condition = None;
        self.max_rounds = None;
        self.checkpoint_each_round = false;
    }

    /// Configure `/loop`-specific fields. Called by the `/loop` command after
    /// the objective is created/activated. Must run inside a synchronous block
    /// (never hold the `Mutex` guard across an `.await`).
    pub fn configure_loop(
        &mut self,
        stop_condition: Option<String>,
        max_rounds: Option<u32>,
        checkpoint_each_round: bool,
    ) {
        self.stop_condition = stop_condition.filter(|value| !value.trim().is_empty());
        self.max_rounds = max_rounds;
        self.checkpoint_each_round = checkpoint_each_round;
    }

    /// Override continuation cap for `decide_continuation` — `None` means the
    /// caller should fall back to `DEFAULT_MAX_CONTINUATIONS`.
    #[must_use]
    pub fn max_continuations_override(&self) -> Option<u32> {
        self.max_rounds
    }

    /// Whether to snapshot the workspace before each continuation round.
    #[must_use]
    pub fn checkpoint_each_round(&self) -> bool {
        self.checkpoint_each_round
    }

    /// The user-defined stop condition, if any.
    #[must_use]
    pub fn stop_condition(&self) -> Option<&str> {
        self.stop_condition.as_deref()
    }

    /// Set or clear the progress checklist shown in the goal contract.
    pub fn set_progress_checklist(&mut self, checklist: Option<String>) {
        self.progress_checklist = checklist.filter(|value| !value.trim().is_empty());
    }

    /// Set the wall-clock budget in seconds (wired into `decide_continuation`).
    pub fn set_time_budget_seconds(&mut self, seconds: Option<u64>) {
        self.time_budget_seconds = seconds;
    }

    /// Apply a per-turn progress signal to the circuit-breaker counters.
    ///
    /// Must be called inside a synchronous block — never hold the `Mutex`
    /// guard across an `.await` (see `ARCHITECTURE_STABILITY.md` §8.3).
    pub fn record_progress_signal(&mut self, signal: &ProgressSignal) {
        if !self.is_active() {
            return;
        }
        match signal {
            ProgressSignal::FileChanged => {
                self.no_progress_rounds = 0;
                self.last_tool_error_fingerprint = None;
                self.repeated_error_rounds = 0;
            }
            ProgressSignal::NoChange => {
                self.no_progress_rounds = self.no_progress_rounds.saturating_add(1);
            }
            ProgressSignal::ToolError { fingerprint } => {
                if self.last_tool_error_fingerprint.as_deref() == Some(fingerprint.as_str()) {
                    self.repeated_error_rounds = self.repeated_error_rounds.saturating_add(1);
                } else {
                    self.repeated_error_rounds = 1;
                    self.last_tool_error_fingerprint = Some(fingerprint.clone());
                }
                // A tool error still counts as a turn that made no file progress.
                self.no_progress_rounds = self.no_progress_rounds.saturating_add(1);
            }
        }
    }

    /// Consecutive no-progress turns (exposed for `decide_continuation`).
    #[must_use]
    pub fn no_progress_rounds(&self) -> u32 {
        self.no_progress_rounds
    }

    /// Consecutive repeated same-error turns (exposed for `decide_continuation`).
    #[must_use]
    pub fn repeated_error_rounds(&self) -> u32 {
        self.repeated_error_rounds
    }

    /// Wall-clock budget in seconds, if set (exposed for `decide_continuation`).
    #[must_use]
    pub fn time_budget_seconds(&self) -> Option<u64> {
        self.time_budget_seconds
    }

    pub fn record_usage(&mut self, token_delta: u64, time_delta_seconds: u64) {
        if self.is_active() {
            self.tokens_used = self.tokens_used.saturating_add(token_delta);
            self.time_used_seconds = self.time_used_seconds.saturating_add(time_delta_seconds);
        }
    }

    pub fn record_continuation(&mut self) {
        if self.is_active() {
            self.continuation_count = self.continuation_count.saturating_add(1);
        }
    }

    pub fn mark_complete(
        &mut self,
        evidence: String,
        verification: GoalCompletionVerification,
    ) -> Result<(), &'static str> {
        if self.objective.is_none() {
            return Err("No active goal exists to complete.");
        }
        self.status = Some(GoalStatus::Complete);
        self.finished_at = Some(Instant::now());
        self.evidence = Some(evidence);
        self.blocker = None;
        self.completion_verification = Some(verification);
        Ok(())
    }

    pub fn mark_blocked(&mut self, blocker: String) -> Result<(), &'static str> {
        if self.objective.is_none() {
            return Err("No active goal exists to block.");
        }
        self.status = Some(GoalStatus::Blocked);
        self.finished_at = Some(Instant::now());
        self.blocker = Some(blocker);
        self.evidence = None;
        self.completion_verification = None;
        Ok(())
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    #[must_use]
    pub fn snapshot(&self) -> GoalSnapshot {
        GoalSnapshot {
            objective: self.objective.clone(),
            status: self
                .status
                .map(GoalStatus::as_str)
                .unwrap_or("none")
                .to_string(),
            token_budget: self.token_budget,
            tokens_used: self.tokens_used,
            time_used_seconds: self.time_used_seconds,
            continuation_count: self.continuation_count,
            elapsed_seconds: self.started_at.map(|started| started.elapsed().as_secs()),
            evidence: self.evidence.clone(),
            blocker: self.blocker.clone(),
            completion_verification: self.completion_verification.clone(),
            progress_checklist: self.progress_checklist.clone(),
            no_progress_rounds: self.no_progress_rounds,
            repeated_error_rounds: self.repeated_error_rounds,
            time_budget_seconds: self.time_budget_seconds,
            stop_condition: self.stop_condition.clone(),
            max_rounds: self.max_rounds,
            checkpoint_each_round: self.checkpoint_each_round,
        }
    }

    /// 从落盘快照重建运行时目标态（best-effort）。
    ///
    /// 快照不含 `Instant` 时间戳，故 `started_at`/`finished_at` 留空——仅用于
    /// 会话重启后的队列兜底恢复，不影响调度语义。
    #[must_use]
    pub fn from_snapshot(snap: &GoalSnapshot) -> Self {
        Self {
            objective: snap.objective.clone(),
            token_budget: snap.token_budget,
            status: Some(GoalStatus::parse_str(&snap.status)),
            tokens_used: snap.tokens_used,
            time_used_seconds: snap.time_used_seconds,
            continuation_count: snap.continuation_count,
            started_at: None,
            finished_at: None,
            evidence: snap.evidence.clone(),
            blocker: snap.blocker.clone(),
            completion_verification: snap.completion_verification.clone(),
            progress_checklist: snap.progress_checklist.clone(),
            no_progress_rounds: snap.no_progress_rounds,
            last_tool_error_fingerprint: None,
            repeated_error_rounds: snap.repeated_error_rounds,
            time_budget_seconds: snap.time_budget_seconds,
            stop_condition: snap.stop_condition.clone(),
            max_rounds: snap.max_rounds,
            checkpoint_each_round: snap.checkpoint_each_round,
            pending_loop_config: None,
        }
    }
}

/// Serializable tool output and prompt input for the current goal.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GoalSnapshot {
    pub objective: Option<String>,
    pub status: String,
    pub token_budget: Option<u32>,
    pub tokens_used: u64,
    pub time_used_seconds: u64,
    pub continuation_count: u32,
    pub elapsed_seconds: Option<u64>,
    pub evidence: Option<String>,
    pub blocker: Option<String>,
    pub completion_verification: Option<GoalCompletionVerification>,
    pub progress_checklist: Option<String>,
    pub no_progress_rounds: u32,
    pub repeated_error_rounds: u32,
    pub time_budget_seconds: Option<u64>,
    /// User-defined stop condition for `/loop`, if set.
    pub stop_condition: Option<String>,
    /// Explicit round cap for `/loop` (`None` = default safety cap).
    pub max_rounds: Option<u32>,
    /// Whether to snapshot the workspace before each continuation round.
    pub checkpoint_each_round: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GoalCompletionVerification {
    pub status: String,
    pub check: String,
    pub summary: String,
}

impl GoalSnapshot {
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.objective.is_some() && self.status == GoalStatus::Active.as_str()
    }

    #[must_use]
    pub fn from_thread_goal(goal: &mimofan_protocol::ThreadGoal) -> Self {
        Self {
            objective: Some(goal.objective.clone()),
            status: thread_goal_status_as_goal_status(goal.status.clone())
                .as_str()
                .to_string(),
            token_budget: goal
                .token_budget
                .and_then(|value| u32::try_from(value.max(0)).ok()),
            tokens_used: u64::try_from(goal.tokens_used.max(0)).unwrap_or(u64::MAX),
            time_used_seconds: u64::try_from(goal.time_used_seconds.max(0)).unwrap_or(u64::MAX),
            continuation_count: u32::try_from(goal.continuation_count.max(0)).unwrap_or(u32::MAX),
            elapsed_seconds: None,
            evidence: None,
            blocker: None,
            completion_verification: None,
            progress_checklist: None,
            no_progress_rounds: 0,
            repeated_error_rounds: 0,
            time_budget_seconds: None,
            stop_condition: None,
            max_rounds: None,
            checkpoint_each_round: false,
        }
    }
}

#[must_use]
pub fn thread_goal_status_as_goal_status(status: mimofan_protocol::ThreadGoalStatus) -> GoalStatus {
    match status {
        mimofan_protocol::ThreadGoalStatus::Active => GoalStatus::Active,
        mimofan_protocol::ThreadGoalStatus::Paused => GoalStatus::Paused,
        mimofan_protocol::ThreadGoalStatus::Complete => GoalStatus::Complete,
        mimofan_protocol::ThreadGoalStatus::Blocked
        | mimofan_protocol::ThreadGoalStatus::UsageLimited
        | mimofan_protocol::ThreadGoalStatus::BudgetLimited => GoalStatus::Blocked,
    }
}

// === Goal queue (multi-objective scheduling) ===
//
// 早期版本是单例 `SharedGoalState`（一个 session 一个 goal，覆盖式写入）。`GoalQueue`
// 将其升级为多目标队列：可连续入队多个 goal，按 **优先级 + 依赖（blocked_by）**
// 串行调度。每个 goal 仍各自持有 `GoalState`（自带 GoalBudget / 进度 / 电路断路器），
// 队列层只负责「选哪个 goal 当前 Active」以及可选的聚合预算护栏。
//
// 沿用 `std::sync::Mutex` 与同步守卫（绝不跨 `.await`，见 ARCHITECTURE_STABILITY.md §8.3）。
// blocked_by 的 DAG 语义复用 `tools/todo.rs` 的思路：缺失的依赖 id 视为已满足（避免
// 删除上游后孤儿死锁），并发边会被丢弃以杜绝成环。

/// 队列层状态机：一个 goal 在队列中的调度位置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueStatus {
    /// 已入队，等待被提升为 Active（依赖未满足或优先级不够时停留在此）。
    Queued,
    /// 当前正在被引擎执行（队列中至多一个）。
    Active,
    /// 被用户暂停，不占用调度槽，可 resume 回 Queued。
    Paused,
    /// 终态：完成 / 阻塞 / 取消。移出调度，但保留在历史中供 `goal_list` 查阅。
    Done,
}

impl QueueStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Active => "active",
            Self::Paused => "paused",
            Self::Done => "done",
        }
    }

    /// 从落盘快照里的字符串还原（默认 `Queued`）。
    #[must_use]
    pub fn parse_str(value: &str) -> Self {
        match value {
            "active" => Self::Active,
            "paused" => Self::Paused,
            "done" => Self::Done,
            _ => Self::Queued,
        }
    }
}

/// 队列中的一个 goal 条目。
#[derive(Debug, Clone)]
pub struct GoalEntry {
    pub id: u32,
    /// 调度优先级（0-255），越大越先被提升为 Active。
    pub priority: u8,
    pub queue_status: QueueStatus,
    /// 该 goal 的运行时状态（预算 / 进度 / 电路断路器），复用单例时代的实现。
    pub goal: GoalState,
    /// 必须率先进入 Done(Complete) 的其他 goal id（其他枚举仅在 Done 后解锁）。
    /// 空或引用不存在的 id 视为无依赖。
    pub blocked_by: Vec<u32>,
}

/// 可序列化的队列条目快照（供 `goal_list` / prompt 注入）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalEntrySnapshot {
    pub id: u32,
    pub priority: u8,
    pub queue_status: String,
    pub objective: Option<String>,
    pub blocked_by: Vec<u32>,
    /// 该条目的 goal 运行时快照（预算消耗等）。
    pub goal: GoalSnapshot,
    /// 当前仍未满足的依赖 id（仅 Queued 时有意义）。
    pub unmet_dependencies: Vec<u32>,
}

/// 可序列化的队列全貌（供 `goal_list`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalQueueSnapshot {
    pub entries: Vec<GoalEntrySnapshot>,
    pub active_id: Option<u32>,
    pub aggregate_token_budget: Option<u64>,
    pub aggregate_tokens_used: u64,
}

/// 多目标 goal 队列。
#[derive(Debug, Clone)]
pub struct GoalQueue {
    entries: Vec<GoalEntry>,
    next_id: u32,
    /// 队列层聚合 token 预算护栏（可选）。所有 goal 的 token 消耗累加后与之比较。
    aggregate_token_budget: Option<u64>,
    aggregate_tokens_used: u64,
}

impl Default for GoalQueue {
    fn default() -> Self {
        // id 从 1 开始（0 保留为「无 id」，便于依赖自检与 self-reference 判断）。
        Self {
            entries: Vec::new(),
            next_id: 1,
            aggregate_token_budget: None,
            aggregate_tokens_used: 0,
        }
    }
}

/// 共享引用，替换旧的 `SharedGoalState`（单例目标态）。
pub type SharedGoalQueue = Arc<Mutex<GoalQueue>>;

/// 创建空队列。
#[must_use]
pub fn new_shared_goal_queue() -> SharedGoalQueue {
    Arc::new(Mutex::new(GoalQueue::default()))
}

/// 从 host 状态种子创建一个已含单目标的队列（供引擎 prompt 注入兼容路径）。
#[must_use]
pub fn new_shared_goal_queue_from_host_status(
    objective: Option<String>,
    token_budget: Option<u32>,
    status: GoalStatus,
) -> SharedGoalQueue {
    let mut queue = GoalQueue::default();
    if objective.as_deref().is_some_and(|o| !o.trim().is_empty()) {
        queue.enqueue(objective.unwrap_or_default(), token_budget, 0, Vec::new());
    } else {
        // 仍给一个空目标条目以承载 host status（与旧 new_shared_goal_state_from_host_status 行为对齐）。
        let mut entry = GoalEntry {
            id: queue.next_id,
            priority: 0,
            queue_status: QueueStatus::Queued,
            goal: GoalState::default(),
            blocked_by: Vec::new(),
        };
        queue.next_id += 1;
        entry.goal.sync_from_host_status(None, token_budget, status);
        // 若 host 要求 Active，则提升为 Active（无依赖）。
        if status == GoalStatus::Active {
            entry.queue_status = QueueStatus::Active;
        }
        queue.entries.push(entry);
    }
    Arc::new(Mutex::new(queue))
}

impl GoalQueue {
    /// 入队一个新 goal。返回其 id。
    ///
    /// - 依赖边会被净化（自引用 / 未知 id / 成环边丢弃），与 todo DAG 一致。
    /// - 若当前无 Active goal，立即尝试提升为新 Active（按优先级与依赖）。
    pub fn enqueue(
        &mut self,
        objective: String,
        token_budget: Option<u32>,
        priority: u8,
        blocked_by: Vec<u32>,
    ) -> u32 {
        let id = self.next_id;
        let blocked_by = self.sanitize_dependencies(id, blocked_by);
        let mut entry = GoalEntry {
            id,
            priority,
            queue_status: QueueStatus::Queued,
            goal: GoalState::default(),
            blocked_by,
        };
        entry.goal.create(objective, token_budget);
        self.next_id += 1;
        self.entries.push(entry);
        if self.active_id().is_none() {
            self.promote_next_ready();
        }
        id
    }

    /// 由 host（`/goal` 命令 / AppAction）同步目标到「active」条目。
    ///
    /// - 若已有 active 条目：在其上 `sync_from_host_status`（保持原语义：同目标不重置、状态切换正确）。
    /// - 若尚无 active 但存在单个 queued 条目（host 直接给了一个目标）：同步到它并提升为 active。
    /// - 否则入队一个新目标并提升为 active。
    pub fn sync_active_from_host(
        &mut self,
        objective: Option<&str>,
        token_budget: Option<u32>,
        status: GoalStatus,
    ) {
        if let Some(id) = self.active_id() {
            if let Some(entry) = self.entries.iter_mut().find(|e| e.id == id) {
                entry
                    .goal
                    .sync_from_host_status(objective, token_budget, status);
            }
            return;
        }
        // 无 active：若只有一个 queued 条目，复用它（host 直接指定了目标）。
        let lone = (self.entries.len() == 1).then(|| self.entries[0].id);
        match lone {
            Some(id) if self.entries[0].queue_status == QueueStatus::Queued => {
                if let Some(entry) = self.entries.iter_mut().find(|e| e.id == id) {
                    entry
                        .goal
                        .sync_from_host_status(objective, token_budget, status);
                    if status == GoalStatus::Active {
                        entry.queue_status = QueueStatus::Active;
                    }
                }
            }
            _ => {
                if objective.is_some_and(|o| !o.trim().is_empty()) {
                    self.enqueue(
                        objective.unwrap_or_default().to_string(),
                        token_budget,
                        0,
                        Vec::new(),
                    );
                } else {
                    // 仅状态变更（如 clear）：清空队列。
                    self.entries.clear();
                    self.next_id = 1;
                    self.aggregate_tokens_used = 0;
                }
            }
        }
    }

    /// 当前 Active goal 的 id（至多一个）。
    #[must_use]
    pub fn active_id(&self) -> Option<u32> {
        self.entries
            .iter()
            .find(|e| e.queue_status == QueueStatus::Active)
            .map(|e| e.id)
    }

    /// 当前 Active goal 条目的可变引用。
    pub fn active_mut(&mut self) -> Option<&mut GoalEntry> {
        let active = self.active_id()?;
        self.entries.iter_mut().find(|e| e.id == active)
    }

    /// 当前 Active goal 的运行时快照（供引擎 prompt 注入 / continuation 决策）。
    #[must_use]
    pub fn active_snapshot(&self) -> Option<GoalSnapshot> {
        let active = self.active_id()?;
        self.entries
            .iter()
            .find(|e| e.id == active)
            .map(|e| e.goal.snapshot())
    }

    /// 取指定 id 的条目引用。
    #[must_use]
    pub fn get(&self, id: u32) -> Option<&GoalEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// 取指定 id 的条目可变引用。
    pub fn get_mut(&mut self, id: u32) -> Option<&mut GoalEntry> {
        self.entries.iter_mut().find(|e| e.id == id)
    }

    /// 指定 id 的运行时快照。
    #[must_use]
    pub fn snapshot_of(&self, id: u32) -> Option<GoalSnapshot> {
        self.get(id).map(|e| e.goal.snapshot())
    }

    /// 队列是否没有任何 goal 条目。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn list_snapshot(&self) -> GoalQueueSnapshot {
        let active_id = self.active_id();
        let entries = self
            .entries
            .iter()
            .map(|e| GoalEntrySnapshot {
                id: e.id,
                priority: e.priority,
                queue_status: e.queue_status.as_str().to_string(),
                objective: e.goal.objective().map(str::to_string),
                blocked_by: e.blocked_by.clone(),
                goal: e.goal.snapshot(),
                unmet_dependencies: self.unmet_dependencies(e.id),
            })
            .collect();
        GoalQueueSnapshot {
            entries,
            active_id,
            aggregate_token_budget: self.aggregate_token_budget,
            aggregate_tokens_used: self.aggregate_tokens_used,
        }
    }

    /// 依赖未满足的 id（引用不存在的 id 视为已满足）。
    #[must_use]
    pub fn unmet_dependencies(&self, id: u32) -> Vec<u32> {
        let Some(entry) = self.get(id) else {
            return Vec::new();
        };
        entry
            .blocked_by
            .iter()
            .copied()
            .filter(|dep| {
                // 不存在的 id 视为已满足（避免删除上游后孤儿死锁）。
                self.get(*dep).is_some_and(|up| {
                    // 仅 Complete 视为依赖达成；Blocked/Cancel 仍未达成。
                    up.goal.status != Some(GoalStatus::Complete)
                })
            })
            .collect()
    }

    /// 该条目是否可以进入 Active（依赖已满足且仍在 Queued）。
    #[must_use]
    pub fn is_ready(&self, id: u32) -> bool {
        matches!(
            self.get(id).map(|e| e.queue_status),
            Some(QueueStatus::Queued)
        ) && self.unmet_dependencies(id).is_empty()
    }

    /// 当无 Active goal 时，从 Queued 项中选「依赖已满足」且 priority 最高
    /// （同优先级按 id 升序，即先入队优先）的项提升为 Active。返回被提升的 id。
    pub fn promote_next_ready(&mut self) -> Option<u32> {
        if self.active_id().is_some() {
            return None;
        }
        // 聚合预算已耗尽则不再提升（护栏）；避免无界续跑。
        if self.aggregate_budget_exhausted() {
            return None;
        }
        let best = self
            .entries
            .iter()
            .filter(|e| e.queue_status == QueueStatus::Queued)
            .filter(|e| self.unmet_dependencies(e.id).is_empty())
            .max_by(|a, b| a.priority.cmp(&b.priority).then(a.id.cmp(&b.id)))
            .map(|e| e.id)?;
        if let Some(entry) = self.entries.iter_mut().find(|e| e.id == best) {
            entry.queue_status = QueueStatus::Active;
            return Some(best);
        }
        None
    }

    /// 手动将某 Queued goal 提前为 Active（仅当当前无 Active 时；否则报错）。
    pub fn promote(&mut self, id: u32) -> Result<u32, String> {
        if self.active_id().is_some() {
            return Err("一个 goal 已处于 active；先 pause 或等待其完成后再 promote".to_string());
        }
        // 先以不可变借用完成校验，再取可变借用，避免 `self` 同时被借两次。
        let Some(entry) = self.get(id) else {
            return Err(format!("goal #{id} 不存在"));
        };
        if entry.queue_status != QueueStatus::Queued {
            return Err(format!(
                "goal #{id} 当前为 {}，无法 promote（仅 queued 可提升）",
                entry.queue_status.as_str()
            ));
        }
        let unmet = self.unmet_dependencies(id);
        if !unmet.is_empty() {
            return Err(format!("goal #{id} 仍被未完成的依赖阻塞：{unmet:?}",));
        }
        let entry = self
            .entries
            .iter_mut()
            .find(|e| e.id == id)
            .expect("goal exists (checked above)");
        entry.queue_status = QueueStatus::Active;
        Ok(id)
    }

    /// 将活动（或指定）goal 标记为 Complete，并触发 promote。
    pub fn mark_complete(
        &mut self,
        id: u32,
        evidence: String,
        verification: GoalCompletionVerification,
    ) -> Result<(), &'static str> {
        let Some(entry) = self.entries.iter_mut().find(|e| e.id == id) else {
            return Err("目标 goal 不存在");
        };
        entry.goal.mark_complete(evidence, verification)?;
        entry.queue_status = QueueStatus::Done;
        self.promote_next_ready();
        Ok(())
    }

    /// 将活动（或指定）goal 标记为 Blocked，并触发 promote。
    pub fn mark_blocked(&mut self, id: u32, blocker: String) -> Result<(), &'static str> {
        let Some(entry) = self.entries.iter_mut().find(|e| e.id == id) else {
            return Err("目标 goal 不存在");
        };
        entry.goal.mark_blocked(blocker)?;
        entry.queue_status = QueueStatus::Done;
        self.promote_next_ready();
        Ok(())
    }

    /// 暂停指定 goal（仅 Active 可暂停；暂停后触发 promote）。
    pub fn pause(&mut self, id: u32) -> Result<(), String> {
        let Some(entry) = self.entries.iter_mut().find(|e| e.id == id) else {
            return Err(format!("goal #{id} 不存在"));
        };
        if entry.queue_status != QueueStatus::Active {
            return Err(format!(
                "goal #{id} 当前为 {}，仅 active 可被 pause",
                entry.queue_status.as_str()
            ));
        }
        entry.queue_status = QueueStatus::Paused;
        self.promote_next_ready();
        Ok(())
    }

    /// 将 Paused goal 恢复为 Queued（重新参与调度）。
    pub fn resume(&mut self, id: u32) -> Result<(), String> {
        let Some(entry) = self.entries.iter_mut().find(|e| e.id == id) else {
            return Err(format!("goal #{id} 不存在"));
        };
        if entry.queue_status != QueueStatus::Paused {
            return Err(format!(
                "goal #{id} 当前为 {}，仅 paused 可被 resume",
                entry.queue_status.as_str()
            ));
        }
        entry.queue_status = QueueStatus::Queued;
        // 若当前无 Active，立即尝试提升（可能就是它自己）。
        if self.active_id().is_none() {
            self.promote_next_ready();
        }
        Ok(())
    }

    /// 取消指定 goal（置为 Done，移出调度），并触发 promote。
    pub fn cancel(&mut self, id: u32) -> Result<(), String> {
        let Some(entry) = self.entries.iter_mut().find(|e| e.id == id) else {
            return Err(format!("goal #{id} 不存在"));
        };
        entry.goal.clear();
        entry.queue_status = QueueStatus::Done;
        self.promote_next_ready();
        Ok(())
    }

    /// 设置队列聚合 token 预算护栏。
    pub fn set_aggregate_token_budget(&mut self, budget: Option<u64>) {
        self.aggregate_token_budget = budget;
    }

    /// 记录 token / 时间消耗（累加到聚合，并委托给 Active goal）。
    pub fn record_usage(&mut self, token_delta: u64, time_delta_seconds: u64) {
        self.aggregate_tokens_used = self.aggregate_tokens_used.saturating_add(token_delta);
        if let Some(entry) = self.active_mut() {
            entry.goal.record_usage(token_delta, time_delta_seconds);
        }
    }

    /// 记录一次续跑（委托给 Active goal）。
    pub fn record_continuation(&mut self) {
        if let Some(entry) = self.active_mut() {
            entry.goal.record_continuation();
        }
    }

    /// 应用进度信号（委托给 Active goal）。
    pub fn record_progress_signal(&mut self, signal: &ProgressSignal) {
        if let Some(entry) = self.active_mut() {
            entry.goal.record_progress_signal(signal);
        }
    }

    /// 聚合预算是否已耗尽（用于阻止进一步 promote）。
    #[must_use]
    pub fn aggregate_budget_exhausted(&self) -> bool {
        self.aggregate_token_budget
            .is_some_and(|b| self.aggregate_tokens_used >= b)
    }

    /// 配置 /loop 专属字段（委托给 Active goal）。
    pub fn configure_loop(
        &mut self,
        stop_condition: Option<String>,
        max_rounds: Option<u32>,
        checkpoint_each_round: bool,
    ) {
        if let Some(entry) = self.active_mut() {
            entry
                .goal
                .configure_loop(stop_condition, max_rounds, checkpoint_each_round);
        }
    }

    /// 将 /loop 配置应用到活动 goal（供 `handle_set_goal_status` 使用）。
    pub fn configure_active_loop(
        &mut self,
        stop_condition: Option<String>,
        max_rounds: Option<u32>,
        checkpoint_each_round: bool,
    ) {
        self.configure_loop(stop_condition, max_rounds, checkpoint_each_round);
    }

    /// 是否应停止续跑（聚合预算耗尽且无 Active 提升空间）。供引擎决策参考。
    #[must_use]
    pub fn should_stop_scheduling(&self) -> bool {
        self.active_id().is_none()
            && self.aggregate_budget_exhausted()
            && !self
                .entries
                .iter()
                .any(|e| e.queue_status == QueueStatus::Queued)
    }

    /// 净化依赖边：丢弃自引用、未知 id、会成环的边。
    fn sanitize_dependencies(&self, id: u32, requested: Vec<u32>) -> Vec<u32> {
        let mut accepted: Vec<u32> = Vec::new();
        for dep in requested {
            if dep == id
                || accepted.contains(&dep)
                || !self.entries.iter().any(|e| e.id == dep)
                || self.depends_on(dep, id)
            {
                continue;
            }
            accepted.push(dep);
        }
        accepted
    }

    /// `from` 是否传递依赖于 `target`（用于环检测）。
    fn depends_on(&self, from: u32, target: u32) -> bool {
        let mut stack = vec![from];
        let mut seen: Vec<u32> = Vec::new();
        while let Some(current) = stack.pop() {
            if current == target {
                return true;
            }
            if seen.contains(&current) {
                continue;
            }
            seen.push(current);
            if let Some(item) = self.entries.iter().find(|e| e.id == current) {
                stack.extend(item.blocked_by.iter().copied());
            }
        }
        false
    }

    /// 从落盘快照重建整个队列（best-effort 兜底加载）。
    ///
    /// 还原条目（id / priority / queue_status / blocked_by / goal 运行时态），
    /// 以及聚合预算与 token 消耗。`next_id` 取所有条目 id 的最大值 +1，避免重建后
    /// 复用旧 id。宿主已注入的内容不应经由此路径覆盖。
    #[must_use]
    pub fn from_snapshot(snapshot: &GoalQueueSnapshot) -> Self {
        let mut queue = GoalQueue::default();
        for entry in &snapshot.entries {
            let goal = GoalState::from_snapshot(&entry.goal);
            queue.entries.push(GoalEntry {
                id: entry.id,
                priority: entry.priority,
                queue_status: QueueStatus::parse_str(&entry.queue_status),
                goal,
                blocked_by: entry.blocked_by.clone(),
            });
            queue.next_id = queue.next_id.max(entry.id.saturating_add(1));
        }
        queue.aggregate_token_budget = snapshot.aggregate_token_budget;
        queue.aggregate_tokens_used = snapshot.aggregate_tokens_used;
        queue
    }
}

/// Render the continuation prompt injected when a goal is still active after a
/// turn. There is no run-level cap, so this shows progress (turn count, tokens)
/// rather than a "N/max" meter — the loop runs until done, blocked, or paused.
///
/// `stop_condition` carries a user-defined natural-language stop predicate for
/// `/loop` (Claude Code `/loop` parity). When `Some`, the model self-judges it
/// each round and stops the loop by calling `goal_update` with `status: "complete"`.
///
/// `base_prompt` is the caller-supplied continuation instruction (kept out of
/// this crate so the pure logic has no prompts/templates dependency).
#[must_use]
pub fn render_goal_continuation_prompt(
    snapshot: &GoalSnapshot,
    continuation_index: u32,
    stop_condition: Option<&str>,
    base_prompt: &str,
) -> String {
    let goal_json = serde_json::to_string_pretty(snapshot).unwrap_or_else(|_| "{}".to_string());
    let stop_section = match stop_condition {
        Some(cond) if !cond.trim().is_empty() => format!(
            "\n\n## Stop Condition (user-defined)\n\n\"{}\"\n\nWhen this condition is met, call `goal_update` with `status: \"complete\"`, concrete evidence that the condition holds, and `verification: {{\"status\":\"passed\",\"check\":\"...\",\"summary\":\"...\"}}` to end the loop. Otherwise continue making progress toward the objective.",
            cond.trim()
        ),
        _ => String::new(),
    };
    format!(
        "{}\n\n## Active Goal State\n\n```json\n{}\n```\n\nContinuation pass #{}.{}{}",
        base_prompt.trim(),
        goal_json,
        continuation_index,
        stop_section,
        if stop_condition.is_none() || stop_condition.is_some_and(|c| c.trim().is_empty()) {
            "If the goal is complete, first run or cite a concrete verifier/check, then call `goal_update` with `status: \"complete\"`, concrete evidence, and `verification: {\"status\":\"passed\",\"check\":\"...\",\"summary\":\"...\"}`. If it is blocked, call `goal_update` with `status: \"blocked\"` and the blocker. Otherwise continue making progress toward the objective."
        } else {
            ""
        },
    )
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
        let prompt = render_goal_continuation_prompt(
            &snap,
            1,
            snap.stop_condition.as_deref(),
            "BASE PROMPT",
        );
        assert!(prompt.contains("build is green"));
        assert!(prompt.contains("Stop Condition"));
        assert!(prompt.contains("BASE PROMPT"));
    }

    #[test]
    fn continuation_prompt_omits_stop_section_without_condition() {
        let s = active_state();
        let snap = s.snapshot();
        let prompt = render_goal_continuation_prompt(&snap, 1, snap.stop_condition.as_deref(), "BASE");
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

    // === 黄金路径快照：固化「外提前」必须保持行为一致 ===

    #[test]
    fn golden_enqueue_snapshot() {
        let q = queue_with_two();
        let snap = q.get(1).unwrap().goal.snapshot();
        let json = serde_json::to_string(&snap).unwrap();
        assert!(json.contains("\"objective\":\"first\""));
        assert!(json.contains("\"status\":\"active\""));
        assert!(json.contains("\"continuation_count\":0"));
        assert!(json.contains("\"no_progress_rounds\":0"));
        assert!(json.contains("\"repeated_error_rounds\":0"));
        assert!(json.contains("\"checkpoint_each_round\":false"));
    }

    #[test]
    fn golden_create_then_record_usage() {
        let mut s = active_state();
        s.record_usage(120, 5);
        s.record_usage(30, 2);
        let snap = s.snapshot();
        assert_eq!(snap.tokens_used, 150);
        assert_eq!(snap.time_used_seconds, 7);
        assert!(snap.elapsed_seconds.is_some());
    }

    #[test]
    fn golden_sync_same_objective_keeps_progress() {
        let mut s = active_state();
        s.record_usage(50, 1);
        s.sync_from_host_status(Some("ship the refactor"), None, GoalStatus::Active);
        assert_eq!(s.snapshot().tokens_used, 50);
        s.sync_from_host_status(None, None, GoalStatus::Active);
        assert!(s.objective().is_none());
        assert!(!s.is_active());
    }

    #[test]
    fn golden_mark_complete_then_blocked_overwrites() {
        let mut s = active_state();
        s.mark_complete(
            "done".to_string(),
            GoalCompletionVerification {
                status: "passed".to_string(),
                check: "x".to_string(),
                summary: "y".to_string(),
            },
        )
        .unwrap();
        assert_eq!(s.snapshot().status, "complete");
        assert_eq!(s.snapshot().evidence.as_deref(), Some("done"));
        // mark_blocked 仅校验 objective 非空（完成态目标仍可被重新标记为 blocked）。
        assert!(s.mark_blocked("stuck".to_string()).is_ok());
        assert_eq!(s.snapshot().status, "blocked");
    }

    #[test]
    fn golden_thread_goal_status_mapping() {
        assert_eq!(
            thread_goal_status_as_goal_status(mimofan_protocol::ThreadGoalStatus::UsageLimited),
            GoalStatus::Blocked
        );
        assert_eq!(
            thread_goal_status_as_goal_status(mimofan_protocol::ThreadGoalStatus::Active),
            GoalStatus::Active
        );
    }

    #[test]
    fn golden_list_snapshot_aggregate() {
        let q = queue_with_two();
        let snap = q.list_snapshot();
        assert_eq!(snap.active_id, Some(1));
        assert_eq!(snap.entries.len(), 2);
        assert_eq!(snap.aggregate_tokens_used, 0);
        assert_eq!(snap.entries[0].unmet_dependencies, Vec::<u32>::new());
    }
}
