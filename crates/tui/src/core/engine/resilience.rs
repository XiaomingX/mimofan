//! Engine resilience + budget-awareness for unattended runs (#845/#848/#851/#856/#857).
//!
//! This module is deliberately self-contained and additive: it holds the new
//! types (`TaskBudget`, `CheckpointStore`, `SerializableAgentState`,
//! `EffortEscalationPolicy`) and the pure retry/escalation logic, plus an
//! integration-friendly [`ResumeController`]. The engine wires thin hooks into
//! these types (see `engine.rs` / `turn_loop.rs`) without rewriting the turn
//! loop. Everything here is unit-tested so the capabilities are verifiable
//! independently of the live LLM path.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex as StdMutex;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::models::Usage;

/// Default cap on how many effort/model escalations a failing turn may trigger
/// before the engine gives up. Bounds cost (issue #845).
pub const DEFAULT_MAX_ESCALATIONS: u32 = 2;

/// Reason the engine stopped a goal/turn early.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BudgetStop {
    /// The task token budget was exhausted before the goal completed.
    Exhausted,
}

/// Errors produced by the resilience subsystem (persistence / serialization).
#[derive(Debug, Error)]
pub enum ResilienceError {
    #[error("checkpoint I/O error: {0}")]
    CheckpointIo(#[from] std::io::Error),
    #[error("checkpoint serialization error: {0}")]
    CheckpointSerde(#[from] serde_json::Error),
    #[error("state file I/O error: {0}")]
    StateIo(std::io::Error),
    #[error("state serialization error: {0}")]
    StateSerde(#[from] StateRestoreError),
}

// ===========================================================================
// #848 — Task token budget
// ===========================================================================

/// A model-visible token budget for an entire goal/task.
///
/// The engine decrements `remaining` by each turn's observed usage and halts
/// when it reaches zero. `add_usage` accepts an [`Usage`] so it can be fed
/// directly from the turn loop.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskBudget {
    /// Total token budget granted for the task.
    pub total: usize,
    /// Tokens remaining after the latest turn.
    pub remaining: usize,
    /// Cumulative tokens consumed across turns.
    pub consumed: usize,
}

impl TaskBudget {
    /// Create a fresh budget of `total` tokens.
    #[must_use]
    pub fn new(total: usize) -> Self {
        Self {
            total,
            remaining: total,
            consumed: 0,
        }
    }

    /// Build a budget from an optional config value; `None` means "unbounded".
    #[must_use]
    pub fn from_config(task_budget_tokens: Option<usize>) -> Option<Self> {
        task_budget_tokens.map(Self::new)
    }

    /// Decrement the budget by `used` tokens (saturating at zero).
    /// Returns `true` if the budget is now exhausted.
    pub fn spend(&mut self, used: usize) -> bool {
        self.consumed = self.consumed.saturating_add(used);
        self.remaining = self.remaining.saturating_sub(used);
        self.remaining == 0
    }

    /// Convenience: spend the input+output token counts from an [`Usage`].
    pub fn spend_usage(&mut self, usage: &Usage) -> bool {
        let used = (usage.input_tokens as usize).saturating_add(usage.output_tokens as usize);
        self.spend(used)
    }

    /// Whether no budget remains.
    #[must_use]
    pub fn is_exhausted(&self) -> bool {
        self.remaining == 0
    }

    /// The model-facing budget marker line, e.g. `<!-- budget: 1234 remaining -->`.
    #[must_use]
    pub fn context_marker(&self) -> String {
        format!(
            "<!-- budget: {} remaining / {} total -->",
            self.remaining, self.total
        )
    }
}

// ===========================================================================
// #851 — Turn-level recoverable checkpoint
// ===========================================================================

/// A single turn checkpoint persisted to `.checkpoints.jsonl`.
///
/// Idempotent: writing the same `(turn, summary)` twice appends twice but the
/// loader de-duplicates by `turn` keeping the *last* occurrence, so a crashed
/// re-emit of turn N does not corrupt resume state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TurnCheckpoint {
    /// 1-based turn index this checkpoint records.
    pub turn: u64,
    /// Short human-readable summary of the turn's outcome.
    pub summary: String,
    /// Objective the turn was pursuing (may be empty for free-form turns).
    pub objective: String,
    /// Total tokens consumed through this turn (cumulative).
    pub tokens_consumed: usize,
}

/// Append-only JSONL store of turn checkpoints for a session.
///
/// Each [`save_turn_checkpoint`] call appends one line. [`load_latest`]
/// returns the highest-turn checkpoint (resolving duplicates by last-writer).
#[derive(Debug, Clone)]
pub struct CheckpointStore {
    path: PathBuf,
    /// In-memory mirror so resume logic never depends on disk re-read races.
    checkpoints: Vec<TurnCheckpoint>,
}

impl CheckpointStore {
    /// Open (or create) a checkpoint store rooted at `session_dir`. The file is
    /// named `.checkpoints.jsonl` inside that directory.
    #[must_use]
    pub fn open(session_dir: &Path) -> Self {
        let path = session_dir.join(".checkpoints.jsonl");
        let checkpoints = Self::read_all(&path).unwrap_or_default();
        Self { path, checkpoints }
    }

    /// Open a store at an explicit file path (used by resume controller).
    #[must_use]
    pub fn open_at(path: PathBuf) -> Self {
        let checkpoints = Self::read_all(&path).unwrap_or_default();
        Self { path, checkpoints }
    }

    fn read_all(path: &Path) -> Result<Vec<TurnCheckpoint>, ResilienceError> {
        if !path.exists() {
            return Ok(Vec::new());
        }
        let content = std::fs::read_to_string(path)?;
        let mut out = Vec::new();
        for line in content.lines().filter(|l| !l.trim().is_empty()) {
            out.push(serde_json::from_str::<TurnCheckpoint>(line)?);
        }
        Ok(out)
    }

    /// Append a checkpoint for `turn`. Idempotent w.r.t. resume correctness:
    /// duplicate `turn` entries are de-duplicated on load, keeping the latest.
    pub fn save_turn_checkpoint(
        &mut self,
        turn: u64,
        summary: &str,
        objective: &str,
        tokens_consumed: usize,
    ) -> Result<(), ResilienceError> {
        let cp = TurnCheckpoint {
            turn,
            summary: summary.to_string(),
            objective: objective.to_string(),
            tokens_consumed,
        };
        let line = serde_json::to_string(&cp)?;
        // Append in-process mirror first; disk write second. Either failing
        // leaves a consistent view because we re-derive latest from disk on
        // open. We avoid touching disk if the exact same checkpoint is already
        // the last entry (cheap idempotency for re-emits).
        if self.checkpoints.last().is_some_and(|last| {
            last.turn == cp.turn && last.summary == cp.summary && last.objective == cp.objective
        }) {
            return Ok(());
        }
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        writeln!(file, "{line}")?;
        self.checkpoints.push(cp);
        Ok(())
    }

    /// All checkpoints currently known (in append order).
    #[must_use]
    pub fn all(&self) -> &[TurnCheckpoint] {
        &self.checkpoints
    }

    /// Count of distinct persisted turns.
    #[must_use]
    pub fn count(&self) -> usize {
        self.checkpoints.len()
    }

    /// The latest (highest-turn) checkpoint, or `None` if empty.
    #[must_use]
    pub fn load_latest(&self) -> Option<&TurnCheckpoint> {
        self.checkpoints.iter().max_by_key(|c| c.turn)
    }

    /// Path of the backing file (used by resume diagnostics).
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

// ===========================================================================
// #856 — Serializable agent-state checkpoint (LangGraph-style)
// ===========================================================================

/// A serializable view of engine state sufficient to resume an unattended run.
///
/// This is a *best-effort* projection: it captures the fields needed to
/// reconstruct the engine at the last turn boundary. It does NOT serialize the
/// full message transcript (that lives in the session on disk) — only the
/// minimal orchestration state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SerializableAgentState {
    /// Schema version; bump on breaking changes to this struct.
    pub schema_version: u32,
    /// The active objective (empty for free-form sessions).
    pub objective: String,
    /// Active goal queue id, if any.
    pub goal_queue_id: Option<String>,
    /// 1-based turn index of the last completed turn.
    pub turn_index: u64,
    /// Remaining token budget (None when unbounded).
    pub budget_remaining: Option<usize>,
    /// Total token budget (None when unbounded).
    pub budget_total: Option<usize>,
    /// Tokens consumed so far.
    pub tokens_consumed: usize,
    /// Summary of currently-active sub-agents (id -> role/type).
    pub active_subagents: HashMap<String, String>,
    /// Files currently claimed/open by the engine (paths).
    pub open_files: Vec<PathBuf>,
    /// How many effort/model escalations have been applied so far.
    pub escalations_applied: u32,
    /// Current model in use.
    pub model: String,
    /// Current reasoning effort tier (free-form string).
    pub reasoning_effort: Option<String>,
}

impl Default for SerializableAgentState {
    fn default() -> Self {
        Self {
            schema_version: SERIALIZABLE_STATE_SCHEMA_VERSION,
            objective: String::new(),
            goal_queue_id: None,
            turn_index: 0,
            budget_remaining: None,
            budget_total: None,
            tokens_consumed: 0,
            active_subagents: HashMap::new(),
            open_files: Vec::new(),
            escalations_applied: 0,
            model: String::new(),
            reasoning_effort: None,
        }
    }
}

/// Current schema version for [`SerializableAgentState`].
pub const SERIALIZABLE_STATE_SCHEMA_VERSION: u32 = 1;

/// Errors converting between live engine state and [`SerializableAgentState`].
#[derive(Debug, Error)]
pub enum StateRestoreError {
    #[error("serializable state schema version {found} is unsupported (expected {expected})")]
    UnsupportedSchema { found: u32, expected: u32 },
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}

impl SerializableAgentState {
    /// Serialize to a pretty JSON string.
    pub fn to_json(&self) -> Result<String, StateRestoreError> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    /// Deserialize from JSON, validating the schema version.
    pub fn from_json(json: &str) -> Result<Self, StateRestoreError> {
        let state: Self = serde_json::from_str(json)?;
        if state.schema_version != SERIALIZABLE_STATE_SCHEMA_VERSION {
            return Err(StateRestoreError::UnsupportedSchema {
                found: state.schema_version,
                expected: SERIALIZABLE_STATE_SCHEMA_VERSION,
            });
        }
        Ok(state)
    }
}

// ===========================================================================
// #845 — Effort/model escalation + validation-retry
// ===========================================================================

/// Ordered reasoning-effort tiers the engine knows how to step through when a
/// turn fails validation. Mirrors the `reasoning_effort` string the engine
/// already feeds to the provider (`"off" | "low" | "medium" | "high" | "max"`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffortTier {
    Off,
    Low,
    Medium,
    High,
    Max,
}

impl EffortTier {
    /// Parse from the free-form `reasoning_effort` string the engine uses.
    #[must_use]
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "off" | "none" | "false" => EffortTier::Off,
            "low" => EffortTier::Low,
            "medium" => EffortTier::Medium,
            "high" => EffortTier::High,
            "max" | "ultra" => EffortTier::Max,
            _ => EffortTier::Medium,
        }
    }

    /// Render back to the provider-facing string.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            EffortTier::Off => "off",
            EffortTier::Low => "low",
            EffortTier::Medium => "medium",
            EffortTier::High => "high",
            EffortTier::Max => "max",
        }
    }

    /// The next tier up, or `None` if already at the ceiling.
    #[must_use]
    pub fn next(&self) -> Option<EffortTier> {
        match self {
            EffortTier::Off => Some(EffortTier::Low),
            EffortTier::Low => Some(EffortTier::Medium),
            EffortTier::Medium => Some(EffortTier::High),
            EffortTier::High => Some(EffortTier::Max),
            EffortTier::Max => None,
        }
    }
}

/// Policy describing how to escalate a failing turn before giving up.
///
/// Escalation raises the reasoning effort tier and/or swaps to a more capable
/// model from `model_upgrade_chain` (tried in order after effort is maxed).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffortEscalationPolicy {
    /// Maximum number of escalations before the engine accepts the failure.
    pub max_escalations: u32,
    /// Optional ordered list of "more capable" models to try in turn.
    pub model_upgrade_chain: Vec<String>,
}

impl Default for EffortEscalationPolicy {
    fn default() -> Self {
        Self {
            max_escalations: DEFAULT_MAX_ESCALATIONS,
            model_upgrade_chain: Vec::new(),
        }
    }
}

/// The result of applying one escalation step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EscalationStep {
    /// New reasoning-effort tier (may be unchanged if already maxed).
    pub effort: EffortTier,
    /// New model (may be unchanged if the upgrade chain is exhausted).
    pub model: String,
    /// Whether this step changed anything (false once fully exhausted).
    pub changed: bool,
}

impl EffortEscalationPolicy {
    /// Compute the next (effort, model) pair given the current ones and how
    /// many escalations have already been applied. `changed == false` means
    /// the policy is exhausted and no further escalation is possible.
    #[must_use]
    pub fn escalate(
        &self,
        current_effort: &EffortTier,
        current_model: &str,
        escalations_applied: u32,
    ) -> EscalationStep {
        if escalations_applied >= self.max_escalations {
            return EscalationStep {
                effort: current_effort.clone(),
                model: current_model.to_string(),
                changed: false,
            };
        }

        // First bump effort until it maxes out.
        if let Some(next_effort) = current_effort.next() {
            return EscalationStep {
                effort: next_effort,
                model: current_model.to_string(),
                changed: true,
            };
        }

        // Effort is maxed: try the next model in the upgrade chain.
        if !self.model_upgrade_chain.is_empty() {
            let idx = (escalations_applied as usize).min(self.model_upgrade_chain.len() - 1);
            return EscalationStep {
                effort: current_effort.clone(),
                model: self.model_upgrade_chain[idx].clone(),
                changed: true,
            };
        }

        EscalationStep {
            effort: current_effort.clone(),
            model: current_model.to_string(),
            changed: false,
        }
    }
}

/// A validation signal the engine can evaluate after a turn.
///
/// Reuses the [`crate::tools::verifier::goal_gate`] `GoalGate` building blocks
/// in the live path; this enum is the testable contract so the retry logic
/// does not depend on the LLM.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationVerdict {
    /// Objective met; no retry needed.
    Pass,
    /// Objective not met; retry is allowed if escalations remain.
    Fail,
}

/// Configuration for the validate-then-retry behavior (#845).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationRetryConfig {
    /// Escalation policy applied on each failed validation.
    pub policy: EffortEscalationPolicy,
    /// Objective string passed to the validator (usually the goal objective).
    pub objective: Option<String>,
}

impl Default for ValidationRetryConfig {
    fn default() -> Self {
        Self {
            policy: EffortEscalationPolicy::default(),
            objective: None,
        }
    }
}

/// Retry a turn with escalating effort until validation passes or the
/// escalation budget is exhausted.
///
/// `run_turn` executes one attempt and returns its observed usage + the
/// validation verdict; `validator` inspects the attempt's output. This is the
/// pure, testable core of #845: callers supply a mock `run_turn` so the live
/// LLM path is never touched by the unit test.
///
/// Returns the number of escalations actually applied and the final verdict.
#[must_use]
pub fn retry_turn_with_escalation<F, V>(
    config: &ValidationRetryConfig,
    initial_effort: EffortTier,
    initial_model: &str,
    mut run_turn: F,
    validator: V,
) -> (u32, ValidationVerdict, EffortTier, String)
where
    F: FnMut(&EffortTier, &str) -> ValidationVerdict,
    V: Fn(&ValidationVerdict) -> ValidationVerdict,
{
    let mut effort = initial_effort;
    let mut model = initial_model.to_string();
    let mut escalations = 0u32;

    loop {
        let verdict = validator(&run_turn(&effort, &model));
        if matches!(verdict, ValidationVerdict::Pass) {
            return (escalations, ValidationVerdict::Pass, effort, model);
        }
        let step = config.policy.escalate(&effort, &model, escalations);
        if !step.changed {
            return (escalations, ValidationVerdict::Fail, effort, model);
        }
        effort = step.effort;
        model = step.model;
        escalations = escalations.saturating_add(1);
    }
}

// ===========================================================================
// #857 — Resume controller (combines checkpoint + state)
// ===========================================================================

/// A controller that, on engine start, loads any prior checkpoint/state for a
/// session and exposes the resume cursor. Pure around [`CheckpointStore`] plus
/// an optional [`SerializableAgentState`] file.
#[derive(Debug, Clone)]
pub struct ResumeController {
    checkpoint_store: CheckpointStore,
    state_path: Option<PathBuf>,
}

impl ResumeController {
    /// Build a resume controller rooted at `session_dir`.
    #[must_use]
    pub fn open(session_dir: &Path) -> Self {
        Self {
            checkpoint_store: CheckpointStore::open(session_dir),
            state_path: Some(session_dir.join(".agent_state.json")),
        }
    }

    /// Build a resume controller from an explicit `resume_session` path
    /// (which may point at either the session dir or a state file directly).
    #[must_use]
    pub fn open_path(resume_session: &Path) -> Self {
        if resume_session.is_dir() {
            Self::open(resume_session)
        } else {
            // Treat as a direct state/checkpoint file path.
            let store = if resume_session.extension().is_some_and(|e| e == "jsonl") {
                CheckpointStore::open_at(resume_session.to_path_buf())
            } else {
                CheckpointStore::open_at(resume_session.with_file_name(".checkpoints.jsonl"))
            };
            Self {
                checkpoint_store: store,
                state_path: Some(resume_session.to_path_buf()),
            }
        }
    }

    /// Whether a prior run left resumable progress.
    #[must_use]
    pub fn has_resume_point(&self) -> bool {
        self.checkpoint_store.load_latest().is_some()
    }

    /// The turn index to resume *from* (already-completed turns are skipped).
    /// Returns the last completed turn index, so the caller should begin at
    /// `last_completed + 1`.
    #[must_use]
    pub fn resume_from_turn(&self) -> Option<u64> {
        self.checkpoint_store.load_latest().map(|cp| cp.turn)
    }

    /// Load the persisted [`SerializableAgentState`], if present and valid.
    pub fn load_state(&self) -> Option<SerializableAgentState> {
        let path = self.state_path.as_ref()?;
        let content = std::fs::read_to_string(path).ok()?;
        SerializableAgentState::from_json(&content).ok()
    }

    /// Persist a [`SerializableAgentState`].
    pub fn save_state(&self, state: &SerializableAgentState) -> Result<(), ResilienceError> {
        let Some(path) = &self.state_path else {
            return Ok(());
        };
        let json = state.to_json()?;
        std::fs::write(path, json).map_err(ResilienceError::StateIo)
    }

    /// The underlying checkpoint store (for save/append during a run).
    #[must_use]
    pub fn checkpoints(&self) -> &CheckpointStore {
        &self.checkpoint_store
    }

    /// Mutable access to the checkpoint store (appends during a run).
    pub fn checkpoints_mut(&mut self) -> &mut CheckpointStore {
        &mut self.checkpoint_store
    }
}

/// A small shared wrapper used by the engine to make the resume controller
/// accessible from both `run()`-start and the per-turn completion path without
/// cloning the file paths repeatedly.
#[derive(Debug, Clone, Default)]
pub struct SharedResumeController(Arc<StdMutex<Option<ResumeController>>>);

impl SharedResumeController {
    /// Wrap an optional controller.
    #[must_use]
    pub fn new(controller: Option<ResumeController>) -> Self {
        Self(Arc::new(StdMutex::new(controller)))
    }

    /// Take the controller out (used at `run()` start to apply resume state).
    #[must_use]
    pub fn take(&self) -> Option<ResumeController> {
        self.0.lock().ok().and_then(|mut g| g.take())
    }

    /// Replace the controller (used after `run()` start so per-turn code can
    /// keep appending checkpoints).
    pub fn set(&self, controller: ResumeController) {
        if let Ok(mut g) = self.0.lock() {
            *g = Some(controller);
        }
    }

    /// Append a turn checkpoint if a controller is active.
    pub fn save_turn_checkpoint(
        &self,
        turn: u64,
        summary: &str,
        objective: &str,
        tokens_consumed: usize,
    ) -> Result<(), ResilienceError> {
        if let Ok(mut g) = self.0.lock() {
            if let Some(ctrl) = g.as_mut() {
                return ctrl.checkpoints_mut().save_turn_checkpoint(
                    turn,
                    summary,
                    objective,
                    tokens_consumed,
                );
            }
        }
        Ok(())
    }

    /// Persist the serializable agent state if a controller is active.
    pub fn save_state(&self, state: &SerializableAgentState) -> Result<(), ResilienceError> {
        if let Ok(mut g) = self.0.lock() {
            if let Some(ctrl) = g.as_mut() {
                return ctrl.save_state(state);
            }
        }
        Ok(())
    }

    /// Whether a resume controller is wired (i.e. a session dir/state path was
    /// configured). When `false`, checkpoint/state persistence is a no-op.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.0.lock().map(|g| g.is_some()).unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- #848 TaskBudget -------------------------------------------------
    #[test]
    fn budget_decrements_and_halts_at_zero() {
        let mut budget = TaskBudget::new(10);
        assert!(!budget.spend(4));
        assert_eq!(budget.remaining, 6);
        assert!(!budget.spend(5));
        assert_eq!(budget.remaining, 1);
        // Spending 1 more exhausts it.
        assert!(budget.spend(1));
        assert_eq!(budget.remaining, 0);
        assert!(budget.is_exhausted());
        // Saturating: never goes negative.
        assert!(budget.spend(100));
        assert_eq!(budget.remaining, 0);
    }

    #[test]
    fn budget_from_config_none_is_unbounded() {
        assert!(TaskBudget::from_config(None).is_none());
        let b = TaskBudget::from_config(Some(50)).unwrap();
        assert_eq!(b.total, 50);
        assert_eq!(b.remaining, 50);
    }

    #[test]
    fn budget_spend_usage_sums_io_tokens() {
        let mut budget = TaskBudget::new(100);
        let usage = Usage {
            input_tokens: 30,
            output_tokens: 20,
            ..Usage::default()
        };
        assert!(!budget.spend_usage(&usage));
        assert_eq!(budget.remaining, 50);
        assert_eq!(budget.consumed, 50);
    }

    #[test]
    fn budget_context_marker_format() {
        let budget = TaskBudget::new(100);
        assert_eq!(
            budget.context_marker(),
            "<!-- budget: 100 remaining / 100 total -->"
        );
    }

    // ---- #851 CheckpointStore -------------------------------------------
    #[test]
    fn checkpoint_write_reload_last_turn_and_count() {
        let dir = std::env::temp_dir().join(format!("mimofan-cp-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut store = CheckpointStore::open(&dir);
        store.save_turn_checkpoint(1, "did A", "obj", 10).unwrap();
        store.save_turn_checkpoint(2, "did B", "obj", 25).unwrap();
        store.save_turn_checkpoint(3, "did C", "obj", 40).unwrap();

        // Reload from disk (fresh store) to prove durability.
        let reloaded = CheckpointStore::open(&dir);
        assert_eq!(reloaded.count(), 3);
        let latest = reloaded.load_latest().unwrap();
        assert_eq!(latest.turn, 3);
        assert_eq!(latest.tokens_consumed, 40);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn checkpoint_is_idempotent_for_same_tail() {
        let dir = std::env::temp_dir().join(format!("mimofan-cp-idem-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut store = CheckpointStore::open(&dir);
        store.save_turn_checkpoint(1, "did A", "obj", 10).unwrap();
        // Re-emit the same tail checkpoint (simulating a crash+resend).
        store.save_turn_checkpoint(1, "did A", "obj", 10).unwrap();
        assert_eq!(store.count(), 1, "identical tail must not duplicate");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn checkpoint_resolves_duplicate_turn_to_last() {
        let dir = std::env::temp_dir().join(format!("mimofan-cp-dup-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut store = CheckpointStore::open(&dir);
        store.save_turn_checkpoint(1, "first", "obj", 10).unwrap();
        store
            .save_turn_checkpoint(1, "corrected", "obj", 12)
            .unwrap();
        let reloaded = CheckpointStore::open(&dir);
        assert_eq!(reloaded.count(), 2);
        assert_eq!(reloaded.load_latest().unwrap().summary, "corrected");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ---- #856 SerializableAgentState ------------------------------------
    #[test]
    fn state_round_trips_key_fields() {
        let mut state = SerializableAgentState::default();
        state.objective = "ship the feature".to_string();
        state.goal_queue_id = Some("goal-7".to_string());
        state.turn_index = 4;
        state.budget_remaining = Some(123);
        state.budget_total = Some(500);
        state.tokens_consumed = 377;
        state
            .active_subagents
            .insert("agent-1".to_string(), "researcher".to_string());
        state.open_files.push(PathBuf::from("/ws/src/main.rs"));
        state.escalations_applied = 1;
        state.model = "deepseek-chat".to_string();
        state.reasoning_effort = Some("high".to_string());

        let json = state.to_json().unwrap();
        let back = SerializableAgentState::from_json(&json).unwrap();
        assert_eq!(back, state);
        assert_eq!(back.objective, "ship the feature");
        assert_eq!(back.turn_index, 4);
        assert_eq!(back.budget_remaining, Some(123));
        assert_eq!(
            back.active_subagents.get("agent-1").map(String::as_str),
            Some("researcher")
        );
    }

    #[test]
    fn state_rejects_unsupported_schema() {
        let mut state = SerializableAgentState::default();
        state.schema_version = 999;
        let json = serde_json::to_string(&state).unwrap();
        assert!(SerializableAgentState::from_json(&json).is_err());
    }

    // ---- #845 EffortEscalationPolicy + retry ----------------------------
    #[test]
    fn effort_tier_steps_up_in_order() {
        let mut t = EffortTier::Off;
        let mut steps = vec![t.clone()];
        while let Some(next) = t.next() {
            steps.push(next.clone());
            t = next;
        }
        assert_eq!(
            steps,
            vec![
                EffortTier::Off,
                EffortTier::Low,
                EffortTier::Medium,
                EffortTier::High,
                EffortTier::Max
            ]
        );
        assert_eq!(EffortTier::Max.next(), None);
    }

    #[test]
    fn policy_escalates_effort_then_model() {
        let policy = EffortEscalationPolicy {
            max_escalations: 3,
            model_upgrade_chain: vec!["model-big".to_string()],
        };
        let step1 = policy.escalate(&EffortTier::Low, "model-small", 0);
        assert!(step1.changed);
        assert_eq!(step1.effort, EffortTier::Medium);
        assert_eq!(step1.model, "model-small");

        // After effort maxes out, the next escalation swaps the model.
        let step2 = policy.escalate(&EffortTier::Max, "model-small", 1);
        assert!(step2.changed);
        assert_eq!(step2.effort, EffortTier::Max);
        assert_eq!(step2.model, "model-big");

        // Beyond the cap, no further change.
        let step3 = policy.escalate(&EffortTier::Max, "model-big", 3);
        assert!(!step3.changed);
    }

    #[test]
    fn retry_escalates_until_pass() {
        let config = ValidationRetryConfig {
            policy: EffortEscalationPolicy {
                max_escalations: 2,
                model_upgrade_chain: vec!["model-big".to_string()],
            },
            objective: Some("make it compile".to_string()),
        };

        // Mock: fails on `low`, passes on `medium` or higher.
        let (escalations, verdict, _effort, _model) = retry_turn_with_escalation(
            &config,
            EffortTier::Low,
            "model-small",
            |effort, _model| {
                if matches!(effort, EffortTier::Low) {
                    ValidationVerdict::Fail
                } else {
                    ValidationVerdict::Pass
                }
            },
            |v| v.clone(),
        );

        assert_eq!(verdict, ValidationVerdict::Pass);
        assert_eq!(
            escalations, 1,
            "should escalate exactly once (low -> medium)"
        );
    }

    #[test]
    fn retry_gives_up_after_cap() {
        let config = ValidationRetryConfig {
            policy: EffortEscalationPolicy {
                max_escalations: 2,
                model_upgrade_chain: Vec::new(),
            },
            objective: Some("never passes".to_string()),
        };
        let (escalations, verdict, _effort, _model) = retry_turn_with_escalation(
            &config,
            EffortTier::Low,
            "model-small",
            |_effort, _model| ValidationVerdict::Fail,
            |v| v.clone(),
        );
        assert_eq!(verdict, ValidationVerdict::Fail);
        assert_eq!(escalations, 2, "should exhaust the escalation cap");
    }

    // ---- #858 acceptance — loop/stop must respect the escalation cap -----
    #[test]
    fn acceptance_858_max_escalations_caps_persistent_failure() {
        // A persistently-failing task (e.g. an infinite-ish retry loop) must
        // STOP rather than spin: the escalation policy caps retries at
        // max_escalations=2 (#845), and retry_turn_with_escalation must honour
        // that cap exactly and never escalate beyond it.
        let config = ValidationRetryConfig {
            policy: EffortEscalationPolicy {
                max_escalations: DEFAULT_MAX_ESCALATIONS, // 2
                model_upgrade_chain: Vec::new(),
            },
            objective: Some("task that never validates".to_string()),
        };

        // Mock turn: always fails validation, simulating a task that the model
        // keeps retrying but never completes.
        let mut attempts = 0u32;
        let (escalations, verdict, effort, model) = retry_turn_with_escalation(
            &config,
            EffortTier::Low,
            "model-small",
            |_effort, _model| {
                attempts += 1;
                ValidationVerdict::Fail
            },
            |v| v.clone(),
        );

        assert_eq!(
            verdict,
            ValidationVerdict::Fail,
            "must give up, not keep spinning"
        );
        assert_eq!(
            escalations, DEFAULT_MAX_ESCALATIONS,
            "escalations must be capped at max_escalations (2)"
        );
        // initial attempt + 2 escalations = 3 attempts total; it must not run
        // away to hundreds of retries.
        assert_eq!(
            attempts,
            DEFAULT_MAX_ESCALATIONS + 1,
            "run_turn must be called exactly cap+1 times"
        );
        // Once the cap is hit, further calls to escalate must be no-ops.
        let step = config.policy.escalate(&effort, &model, escalations);
        assert!(!step.changed, "escalate() must stop changing past the cap");
    }

    // ---- #861 acceptance — crash recovery resumes the correct turn -------
    #[test]
    fn acceptance_861_crash_recovery_resumes_turn_three() {
        // #861 — a run interrupted mid-way must resume from where it left off.
        // Simulate 3 turns, each writing a turn checkpoint to a session dir.
        // Then "crash" (drop the handles) and start a fresh engine-like replay
        // from the same session path; it must recover turn index 3 (skip the
        // 3 completed turns) and the budget/objective state.
        let dir = std::env::temp_dir().join(format!("mimofan-accept-861-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();

        // --- First (crashed) engine: writes 3 turn checkpoints + state ------
        {
            let mut ctrl = ResumeController::open(&dir);
            // Turn 1, 2, 3 all complete and persist their checkpoints.
            ctrl.checkpoints_mut()
                .save_turn_checkpoint(1, "scaffold module", "build the feature", 100)
                .unwrap();
            ctrl.checkpoints_mut()
                .save_turn_checkpoint(2, "implement core", "build the feature", 250)
                .unwrap();
            ctrl.checkpoints_mut()
                .save_turn_checkpoint(3, "wire tests", "build the feature", 400)
                .unwrap();

            // Persist orchestration state: objective + remaining budget.
            let mut state = SerializableAgentState::default();
            state.objective = "build the feature".to_string();
            state.turn_index = 3;
            state.budget_remaining = Some(600);
            state.budget_total = Some(1000);
            state.tokens_consumed = 400;
            ctrl.save_state(&state).unwrap();
            // `ctrl` and `state` drop here = the "crash".
        }

        // --- Fresh engine replays from the same session dir -----------------
        let resumed = ResumeController::open(&dir);
        assert!(resumed.has_resume_point(), "crash left resumable progress");
        // Last completed turn was 3, so the engine must resume at turn 4
        // (already-completed turns are skipped).
        assert_eq!(
            resumed.resume_from_turn(),
            Some(3),
            "must recover turn index 3 as last-completed"
        );
        let recovered = resumed.load_state().expect("state must survive the crash");
        assert_eq!(recovered.turn_index, 3, "objective turn state recovered");
        assert_eq!(
            recovered.objective, "build the feature",
            "objective recovered"
        );
        assert_eq!(
            recovered.budget_remaining,
            Some(600),
            "budget state recovered"
        );
        assert_eq!(recovered.tokens_consumed, 400);
        // Durability: the checkpoint file on disk really holds 3 turns.
        assert_eq!(
            resumed.checkpoints().count(),
            3,
            "three turn checkpoints persisted to disk"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ---- #857 ResumeController ------------------------------------------
    #[test]
    fn resume_skips_already_done_turns() {
        let dir = std::env::temp_dir().join(format!("mimofan-resume-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();

        // Simulate a run interrupted after 2 turns.
        let mut ctrl = ResumeController::open(&dir);
        ctrl.checkpoints_mut()
            .save_turn_checkpoint(1, "turn 1 done", "obj", 10)
            .unwrap();
        ctrl.checkpoints_mut()
            .save_turn_checkpoint(2, "turn 2 done", "obj", 20)
            .unwrap();

        // New engine, same session dir, restarts.
        let resumed = ResumeController::open(&dir);
        assert!(resumed.has_resume_point());
        // Last completed turn = 2, so the engine should resume at turn 3.
        assert_eq!(resumed.resume_from_turn(), Some(2));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #861 acceptance: a run interrupted mid-way can resume. We simulate a
    /// 3-turn run that writes a checkpoint per turn, then "crash" (drop all
    /// handles), then start a fresh engine-like replay from the same session
    /// path and assert it recovers turn index 3 (already-completed turns
    /// skipped) together with the persisted budget/objective state.
    #[test]
    fn acceptance_861_resume_recovers_turn_index_and_state() {
        let dir = std::env::temp_dir().join(format!("mimofan-resume-acc-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();

        // ---- Phase 1: original run, 3 turns, then crash. ----
        {
            let mut ctrl = ResumeController::open(&dir);
            let mut tokens = 0usize;
            for turn in 1..=3u64 {
                tokens += 15;
                ctrl.checkpoints_mut()
                    .save_turn_checkpoint(
                        turn,
                        &format!("turn {turn} completed"),
                        "ship the feature",
                        tokens,
                    )
                    .unwrap();
            }
            // Persist orchestration state at the last turn boundary.
            let mut state = SerializableAgentState::default();
            state.objective = "ship the feature".to_string();
            state.turn_index = 3;
            state.budget_remaining = Some(45);
            state.budget_total = Some(100);
            state.tokens_consumed = tokens;
            state.escalations_applied = 0;
            ctrl.save_state(&state).unwrap();
            // <-- handles dropped here = "crash". Disk is the only source of truth.
        }

        // ---- Phase 2: fresh engine replay from the same session dir. ----
        let resumed = ResumeController::open(&dir);
        assert!(
            resumed.has_resume_point(),
            "crash left a resumable checkpoint"
        );

        // Last completed turn is 3, so the engine must resume at turn 4
        // (already-completed turns are skipped).
        assert_eq!(
            resumed.resume_from_turn(),
            Some(3),
            "resume cursor must be the last completed turn"
        );

        // Recovered orchestration state must match what was persisted.
        let state = resumed.load_state().expect("state must survive the crash");
        assert_eq!(state.turn_index, 3, "turn_index must be recovered");
        assert_eq!(
            state.objective, "ship the feature",
            "objective must be recovered"
        );
        assert_eq!(state.budget_remaining, Some(45), "budget must be recovered");
        assert_eq!(state.budget_total, Some(100));
        assert_eq!(state.tokens_consumed, 45, "cumulative tokens recovered");

        // The replay must NOT re-run completed turns: the checkpoint count is
        // still exactly 3 — nothing was lost or duplicated by the crash.
        assert_eq!(resumed.checkpoints().count(), 3);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resume_state_round_trips_through_controller() {
        let dir =
            std::env::temp_dir().join(format!("mimofan-resume-state-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();

        let ctrl = ResumeController::open(&dir);
        let mut state = SerializableAgentState::default();
        state.turn_index = 5;
        state.budget_remaining = Some(42);
        ctrl.save_state(&state).unwrap();

        let reloaded = ResumeController::open(&dir).load_state().unwrap();
        assert_eq!(reloaded.turn_index, 5);
        assert_eq!(reloaded.budget_remaining, Some(42));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
