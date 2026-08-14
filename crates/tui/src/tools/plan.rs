//! Plan tool implementation with step tracking and validation

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_json::json;

use crate::tools::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec,
};

// === Types ===

/// Status of a plan step.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    #[default]
    Pending,
    InProgress,
    Completed,
}

impl StepStatus {
    #[must_use]
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(value: &str) -> Option<Self> {
        match value.trim().to_lowercase().as_str() {
            "pending" => Some(StepStatus::Pending),
            "in_progress" | "inprogress" => Some(StepStatus::InProgress),
            "completed" | "done" => Some(StepStatus::Completed),
            _ => None,
        }
    }
}

/// Input representation for a plan item.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlanItemArg {
    pub step: String,
    pub status: StepStatus,
    /// Ids of other plan steps that must complete before this one (nac-style
    /// `depends_on`). Optional; ignored for ordering today but persisted for
    /// display and future scheduling.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub depends_on: Vec<String>,
    /// Scope this step owns (file/module/system boundary), for non-overlapping
    /// ownership. Optional.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    /// Role this step plays (research/implementation/verification/...). Optional.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// Acceptance criteria the step must satisfy to be considered done. Phrases
    /// wrapped in double or single quotes are treated as required evidence
    /// substrings by the step gate. Optional.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acceptance: Option<String>,
    /// Evidence (command output / test result / diff) proving the step is done,
    /// supplied when marking the step completed. Evaluated against `acceptance`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<String>,
}

/// Update payload used by the plan tool.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpdatePlanArgs {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub objective: Option<String>,
    #[serde(default)]
    pub context_summary: Option<String>,
    #[serde(default)]
    pub explanation: Option<String>,
    #[serde(default)]
    pub sources_used: Vec<String>,
    #[serde(default)]
    pub critical_files: Vec<String>,
    #[serde(default)]
    pub constraints: Vec<String>,
    #[serde(default)]
    pub recommended_approach: Option<String>,
    #[serde(default)]
    pub verification_plan: Option<String>,
    #[serde(default)]
    pub risks_and_unknowns: Option<String>,
    #[serde(default)]
    pub handoff_packet: Option<String>,
    #[serde(default)]
    pub plan: Vec<PlanItemArg>,
}

// === Plan State ===

/// A plan step with timing information
#[derive(Debug, Clone)]
pub struct PlanStep {
    pub text: String,
    pub status: StepStatus,
    /// When the step was started (transitioned to `InProgress`)
    pub started_at: Option<Instant>,
    /// When the step was completed
    pub completed_at: Option<Instant>,
    /// Ids of other steps that must complete before this one.
    pub depends_on: Vec<String>,
    /// Scope this step owns (file/module/system boundary).
    pub scope: Option<String>,
    /// Role this step plays (research/implementation/verification/...).
    pub role: Option<String>,
    /// Acceptance criteria text the step must satisfy to be done.
    pub acceptance: Option<String>,
    /// Evidence supplied when the step was marked completed.
    pub evidence: Option<String>,
    /// Whether the step gate (acceptance verification) failed.
    pub verification_failed: bool,
}

impl PlanStep {
    /// Create a new plan step.
    pub fn new(text: String, status: StepStatus) -> Self {
        Self {
            text,
            status,
            started_at: None,
            completed_at: None,
            depends_on: Vec::new(),
            scope: None,
            role: None,
            acceptance: None,
            evidence: None,
            verification_failed: false,
        }
    }

    /// Get the elapsed time if the step has timing info
    #[must_use]
    pub fn elapsed(&self) -> Option<Duration> {
        match (self.started_at, self.completed_at) {
            (Some(start), Some(end)) => Some(end.duration_since(start)),
            (Some(start), None) if self.status == StepStatus::InProgress => Some(start.elapsed()),
            _ => None,
        }
    }

    /// Format elapsed time for display
    #[must_use]
    pub fn elapsed_str(&self) -> String {
        match self.elapsed() {
            Some(d) => {
                let secs = d.as_secs();
                if secs < 60 {
                    format!("{secs}s")
                } else if secs < 3600 {
                    format!("{}m {}s", secs / 60, secs % 60)
                } else {
                    format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
                }
            }
            None => String::new(),
        }
    }
}

/// Serializable snapshot for display
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlanSnapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub objective: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explanation: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources_used: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub critical_files: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constraints: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommended_approach: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_plan: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub risks_and_unknowns: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handoff_packet: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<PlanItemArg>,
}

impl PlanSnapshot {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.objective.is_none()
            && self.context_summary.is_none()
            && self.explanation.is_none()
            && self.sources_used.is_empty()
            && self.critical_files.is_empty()
            && self.constraints.is_empty()
            && self.recommended_approach.is_none()
            && self.verification_plan.is_none()
            && self.risks_and_unknowns.is_none()
            && self.handoff_packet.is_none()
            && self.items.is_empty()
    }

    /// Parse the user/model-facing `update_plan` payload into a displayable
    /// snapshot. This is intentionally tolerant so saved transcript replay can
    /// keep legacy and partially streamed payloads visible.
    #[must_use]
    pub fn from_tool_input(input: &serde_json::Value) -> Self {
        let mut items = Vec::new();
        if let Some(plan_items) = input.get("plan").and_then(|v| v.as_array()) {
            for item in plan_items {
                let step = item
                    .get("step")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .unwrap_or("");
                if step.is_empty() {
                    continue;
                }
                let status = item
                    .get("status")
                    .and_then(|v| v.as_str())
                    .and_then(StepStatus::from_str)
                    .unwrap_or(StepStatus::Pending);
                let depends_on = item
                    .get("depends_on")
                    .and_then(Value::as_array)
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str())
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let scope = item.get("scope").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
                let role = item.get("role").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
                let acceptance = item.get("acceptance").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
                let evidence = item.get("evidence").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
                items.push(PlanItemArg {
                    step: step.to_string(),
                    status,
                    depends_on,
                    scope,
                    role,
                    acceptance,
                    evidence,
                });
            }
        }

        Self {
            title: clean_optional(string_field(input, "title")),
            objective: clean_optional(string_field(input, "objective")),
            context_summary: clean_optional(string_field(input, "context_summary")),
            explanation: clean_optional(string_field(input, "explanation")),
            sources_used: clean_list(string_vec_field(input, "sources_used")),
            critical_files: clean_list(string_vec_field(input, "critical_files")),
            constraints: clean_list(string_vec_field(input, "constraints")),
            recommended_approach: clean_optional(string_field(input, "recommended_approach")),
            verification_plan: clean_optional(string_field(input, "verification_plan")),
            risks_and_unknowns: clean_optional(string_field(input, "risks_and_unknowns")),
            handoff_packet: clean_optional(string_field(input, "handoff_packet")),
            items,
        }
    }
}

/// State tracking for the current plan
#[derive(Debug, Clone, Default)]
pub struct PlanState {
    title: Option<String>,
    objective: Option<String>,
    context_summary: Option<String>,
    explanation: Option<String>,
    sources_used: Vec<String>,
    critical_files: Vec<String>,
    constraints: Vec<String>,
    recommended_approach: Option<String>,
    verification_plan: Option<String>,
    risks_and_unknowns: Option<String>,
    handoff_packet: Option<String>,
    steps: Vec<PlanStep>,
    /// Plan text the user explicitly approved via `exit_plan_mode`, if any.
    /// Recorded so later turns can tell an approved plan apart from a draft
    /// the user never signed off on.
    approved_plan: Option<String>,
    /// Optional on-disk checkpoint path. When set, every [`PlanState::update`]
    /// persists a [`PlanSnapshot`] here so a subsequent process restart can
    /// resume from the last checkpoint (cross-process durability, not just
    /// in-session memory). `None` keeps the legacy in-memory-only behaviour.
    persist_path: Option<PathBuf>,
}

impl PlanState {
    /// Record the plan text the user approved when leaving Plan mode.
    pub fn set_approved_plan(&mut self, plan: String) {
        self.approved_plan = Some(plan);
    }

    /// The plan the user approved, if they have approved one.
    #[must_use]
    pub fn approved_plan(&self) -> Option<&str> {
        self.approved_plan.as_deref()
    }

    /// Check whether the plan is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
            && self.title.is_none()
            && self.objective.is_none()
            && self.context_summary.is_none()
            && self.explanation.is_none()
            && self.sources_used.is_empty()
            && self.critical_files.is_empty()
            && self.constraints.is_empty()
            && self.recommended_approach.is_none()
            && self.verification_plan.is_none()
            && self.risks_and_unknowns.is_none()
            && self.handoff_packet.is_none()
    }

    pub fn update(&mut self, args: UpdatePlanArgs) {
        self.title = clean_optional(args.title);
        self.objective = clean_optional(args.objective);
        self.context_summary = clean_optional(args.context_summary);
        self.explanation = clean_optional(args.explanation);
        self.sources_used = clean_list(args.sources_used);
        self.critical_files = clean_list(args.critical_files);
        self.constraints = clean_list(args.constraints);
        self.recommended_approach = clean_optional(args.recommended_approach);
        self.verification_plan = clean_optional(args.verification_plan);
        self.risks_and_unknowns = clean_optional(args.risks_and_unknowns);
        self.handoff_packet = clean_optional(args.handoff_packet);

        let now = Instant::now();
        let mut new_steps = Vec::new();
        let mut in_progress_seen = false;

        for item in args.plan {
            let step_text = item.step.trim();
            if step_text.is_empty() {
                continue;
            }
            // Try to find existing step to preserve timing
            let existing = self.steps.iter().find(|s| s.text == step_text);

            let mut status = item.status;
            // Enforce single in_progress
            if status == StepStatus::InProgress {
                if in_progress_seen {
                    status = StepStatus::Pending;
                } else {
                    in_progress_seen = true;
                }
            }

            // Resolve structured per-item fields: prefer the value supplied in
            // this update; for an existing step, fall back to the previously
            // stored value so a partial update doesn't wipe earlier metadata.
            let depends_on = if item.depends_on.is_empty() {
                existing
                    .map(|e| e.depends_on.clone())
                    .unwrap_or_default()
            } else {
                item.depends_on.clone()
            };
            let scope = item
                .scope
                .clone()
                .or_else(|| existing.and_then(|e| e.scope.clone()));
            let role = item
                .role
                .clone()
                .or_else(|| existing.and_then(|e| e.role.clone()));
            let acceptance = item
                .acceptance
                .clone()
                .or_else(|| existing.and_then(|e| e.acceptance.clone()));
            // Evidence is only meaningful when completing a step; always take the
            // freshly supplied value (or clear it on a non-completed update).
            let evidence = if status == StepStatus::Completed {
                item.evidence.clone()
            } else {
                None
            };

            let step = if let Some(old) = existing {
                let mut s = old.clone();
                let old_status = s.status.clone();
                s.status = status.clone();
                s.depends_on = depends_on;
                s.scope = scope;
                s.role = role;
                s.acceptance = acceptance;
                s.evidence = evidence;
                // Re-evaluate the gate whenever re-completed with new evidence.
                if status == StepStatus::Completed {
                    s.verification_failed = false;
                }

                // Track timing transitions
                if old_status == StepStatus::Pending && status == StepStatus::InProgress {
                    s.started_at = Some(now);
                }
                if old_status == StepStatus::InProgress && status == StepStatus::Completed {
                    s.completed_at = Some(now);
                }

                s
            } else {
                let mut s = PlanStep::new(step_text.to_string(), status.clone());
                s.depends_on = depends_on;
                s.scope = scope;
                s.role = role;
                s.acceptance = acceptance;
                s.evidence = evidence;
                if status == StepStatus::InProgress {
                    s.started_at = Some(now);
                }
                s
            };

            new_steps.push(step);
        }

        self.steps = new_steps;

        // Cross-process checkpoint: persist the snapshot to disk on every
        // mutation so a later process restart can resume from this point.
        self.try_persist();
    }

    pub fn snapshot(&self) -> PlanSnapshot {
        PlanSnapshot {
            title: self.title.clone(),
            objective: self.objective.clone(),
            context_summary: self.context_summary.clone(),
            explanation: self.explanation.clone(),
            sources_used: self.sources_used.clone(),
            critical_files: self.critical_files.clone(),
            constraints: self.constraints.clone(),
            recommended_approach: self.recommended_approach.clone(),
            verification_plan: self.verification_plan.clone(),
            risks_and_unknowns: self.risks_and_unknowns.clone(),
            handoff_packet: self.handoff_packet.clone(),
            items: self
                .steps
                .iter()
                .map(|s| PlanItemArg {
                    step: s.text.clone(),
                    status: s.status.clone(),
                    depends_on: s.depends_on.clone(),
                    scope: s.scope.clone(),
                    role: s.role.clone(),
                    acceptance: s.acceptance.clone(),
                    evidence: s.evidence.clone(),
                })
                .collect(),
        }
    }

    /// Rebuild the plan state from a previously persisted [`PlanSnapshot`]
    /// (e.g. restored from a saved session). Step timing is intentionally not
    /// restored — only text and status are durable in the snapshot.
    pub fn apply_snapshot(&mut self, snap: PlanSnapshot) {
        self.title = snap.title;
        self.objective = snap.objective;
        self.context_summary = snap.context_summary;
        self.explanation = snap.explanation;
        self.sources_used = snap.sources_used;
        self.critical_files = snap.critical_files;
        self.constraints = snap.constraints;
        self.recommended_approach = snap.recommended_approach;
        self.verification_plan = snap.verification_plan;
        self.risks_and_unknowns = snap.risks_and_unknowns;
        self.handoff_packet = snap.handoff_packet;
        self.steps = snap
            .items
            .into_iter()
            .filter(|item| !item.step.trim().is_empty())
            .map(|item| {
                let mut s = PlanStep::new(item.step, item.status);
                s.depends_on = item.depends_on.clone();
                s.scope = item.scope.clone();
                s.role = item.role.clone();
                s.acceptance = item.acceptance.clone();
                s.evidence = item.evidence.clone();
                s
            })
            .collect();
    }

    /// Configure the on-disk checkpoint path. Once set, every [`PlanState::update`]
    /// writes a [`PlanSnapshot`] here so a later process restart can resume.
    pub fn set_persist_path(&mut self, path: PathBuf) {
        self.persist_path = Some(path);
    }

    /// Persist the current snapshot to `path` (overwriting). Errors are
    /// returned to the caller, which may decide how to surface them.
    pub fn persist_to(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        let snap = self.snapshot();
        let json = serde_json::to_string_pretty(&snap)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        fs::write(path, json)
    }

    /// Best-effort persist: used after every `update`. Failures are logged and
    /// swallowed so a disk hiccup never aborts a plan update (the in-memory
    /// state is still authoritative within the session).
    pub fn try_persist(&self) {
        if let Some(path) = &self.persist_path {
            if let Err(e) = self.persist_to(path) {
                tracing::warn!("plan checkpoint persist to {} failed: {e}", path.display());
            }
        }
    }

    /// Restore a previously persisted checkpoint into this state, if the file
    /// exists and parses. Returns `true` when a checkpoint was loaded. A
    /// missing or corrupt file is treated as "no checkpoint yet" and leaves the
    /// state untouched (the in-memory default is valid).
    pub fn restore_if_present(&mut self) -> bool {
        let Some(path) = self.persist_path.clone() else {
            return false;
        };
        if !path.exists() {
            return false;
        }
        let Ok(json) = fs::read_to_string(&path) else {
            tracing::warn!("plan checkpoint read {} failed; skipping restore", path.display());
            return false;
        };
        match serde_json::from_str::<PlanSnapshot>(&json) {
            Ok(snap) => {
                self.apply_snapshot(snap);
                true
            }
            Err(e) => {
                tracing::warn!(
                    "plan checkpoint {} corrupted ({}); starting fresh",
                    path.display(),
                    e
                );
                false
            }
        }
    }

    pub fn explanation(&self) -> Option<&str> {
        self.explanation.as_deref()
    }

    pub fn steps(&self) -> &[PlanStep] {
        &self.steps
    }

    /// Get counts of steps by status
    pub fn counts(&self) -> (usize, usize, usize) {
        let mut pending = 0;
        let mut in_progress = 0;
        let mut completed = 0;
        for s in &self.steps {
            match s.status {
                StepStatus::Pending => pending += 1,
                StepStatus::InProgress => in_progress += 1,
                StepStatus::Completed => completed += 1,
            }
        }
        (pending, in_progress, completed)
    }

    /// Get progress as a percentage
    pub fn progress_percent(&self) -> u8 {
        if self.steps.is_empty() {
            return 0;
        }
        let completed = self
            .steps
            .iter()
            .filter(|s| s.status == StepStatus::Completed)
            .count();
        let percent = completed.saturating_mul(100) / self.steps.len();
        u8::try_from(percent).unwrap_or(u8::MAX)
    }
}

fn clean_optional(value: Option<String>) -> Option<String> {
    value
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

// === Plan deviation detection ===
//
// Once the user approves a plan (via `exit_plan_mode`), later turns must not
// silently wander off it. `detect_deviation` compares the approved plan with
// the steps the model is actually executing and reports every step that
// drifts from the approved plan (off-plan / violates the approved plan) so the
// engine can surface the mismatch before more work is wasted.

/// A single place where execution deviated from the approved plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanDeviation {
    /// The plan step the execution diverged from, if it maps to one.
    pub planned_step: Option<String>,
    /// The actual executed step that did not match the plan.
    pub actual_step: String,
    /// Short human-readable description of how the step `drift_from_plan`.
    pub detail: String,
}

/// The approved plan, as a set of ordered step texts agreed with the user.
#[derive(Debug, Clone, Default)]
pub struct Plan {
    pub steps: Vec<String>,
}

impl Plan {
    /// Build a `Plan` from the approved-plan text. Steps are split on
    /// newlines and de-bulleted so the comparison is order- and formatting
    /// tolerant.
    #[must_use]
    pub fn from_approved(approved: &str) -> Self {
        let steps = approved
            .lines()
            .map(|line| line.trim())
            .filter(|line| !line.is_empty())
            .map(|line| {
                let trimmed = line
                    .trim_start_matches([
                        '-', '*', '+', '#', '1', '2', '3', '4', '5', '6', '7', '8', '9', '0',
                    ])
                    .trim_start_matches('.')
                    .trim()
                    .to_string();
                trimmed
            })
            .filter(|line| !line.is_empty())
            .collect();
        Self { steps }
    }
}

/// The steps actually executed so far this session.
#[derive(Debug, Clone, Default)]
pub struct ExecutedSteps {
    pub steps: Vec<String>,
}

impl ExecutedSteps {
    /// Record one executed step.
    pub fn record(&mut self, step: String) {
        self.steps.push(step);
    }
}

/// Compare the approved [`Plan`] against the [`ExecutedSteps`] and return every
/// deviation. A step `drift_from_plan` when an executed step has no
/// sufficiently similar counterpart in the approved plan, or when the plan
/// contains a step that execution never reached.
///
/// Similarity is a cheap token-overlap check (Jaccard) so minor wording
/// differences in the model's own re-statement of a step do not count as a
/// violation.
#[must_use]
pub fn detect_deviation(planned: &Plan, actual: &ExecutedSteps) -> Vec<PlanDeviation> {
    let mut deviations = Vec::new();

    // Off-plan: executed steps with no match in the approved plan.
    for executed in &actual.steps {
        let matched = planned
            .steps
            .iter()
            .any(|p| step_similarity(p, executed) >= 0.5);
        if !matched {
            deviations.push(PlanDeviation {
                planned_step: None,
                actual_step: executed.clone(),
                detail: format!(
                    "executed step is off_plan: no approved plan step matches `{executed}`"
                ),
            });
        }
    }

    // Missing: approved steps the execution never reached.
    let reached = |planned_step: &str| -> bool {
        actual
            .steps
            .iter()
            .any(|a| step_similarity(planned_step, a) >= 0.5)
    };
    for planned_step in &planned.steps {
        if !reached(planned_step) {
            deviations.push(PlanDeviation {
                planned_step: Some(planned_step.clone()),
                actual_step: String::new(),
                detail: format!(
                    "approved plan step not executed: `{planned_step}` violates_plan expectation"
                ),
            });
        }
    }

    deviations
}

/// Jaccard token overlap between two step descriptions (whitespace tokens).
fn step_similarity(a: &str, b: &str) -> f64 {
    let set_a: std::collections::HashSet<&str> = a.split_whitespace().collect();
    let set_b: std::collections::HashSet<&str> = b.split_whitespace().collect();
    if set_a.is_empty() && set_b.is_empty() {
        return 1.0;
    }
    let intersection = set_a.intersection(&set_b).count();
    let union = set_a.union(&set_b).count();
    if union == 0 {
        return 0.0;
    }
    intersection as f64 / union as f64
}

// === Step verification (acceptance gating) ===
//
// Each plan step can carry `acceptance_criteria`: the observable evidence that
// must be present before the step is considered done. `verify_step` gates a
// step by checking the supplied evidence (tool output / test result / diff)
// against those criteria, returning a structured [`StepVerification`] that the
// engine uses as a `step_gate` before marking the step completed.

/// Acceptance criteria a plan step must satisfy to pass its `step_gate`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AcceptanceCriteria {
    /// Human-readable criteria text the model committed to when planning.
    pub criteria: String,
    /// Optional keyword/phrase that must appear in the evidence to pass.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_substrings: Vec<String>,
}

/// Outcome of gating a plan step against its evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepVerification {
    /// Whether the evidence satisfies the step's `acceptance_criteria`.
    pub passed: bool,
    /// The reason a step_gate failed (empty when it passed).
    pub reason: String,
    /// The observed evidence that was evaluated.
    pub evidence: String,
}

/// Gate a single [`PlanStep`] against the evidence collected so far.
///
/// `acceptance_criteria` is the contract for the step; `evidence` is whatever
/// the model supplied to prove the step is done (command output, test pass
/// line, diff). When no criteria are specified the step auto-passes (trust the
/// model's own status) so legacy plans are not blocked.
#[must_use]
pub fn verify_step(
    step: &PlanStep,
    acceptance_criteria: &AcceptanceCriteria,
    evidence: &str,
) -> StepVerification {
    if acceptance_criteria.criteria.trim().is_empty()
        && acceptance_criteria.required_substrings.is_empty()
    {
        return StepVerification {
            passed: true,
            reason: String::new(),
            evidence: evidence.to_string(),
        };
    }

    let missing: Vec<&String> = acceptance_criteria
        .required_substrings
        .iter()
        .filter(|needle| !evidence.contains(needle.as_str()))
        .collect();
    if !missing.is_empty() {
        return StepVerification {
            passed: false,
            reason: format!(
                "step `{}` step_gate failed: evidence missing required substring(s) {}",
                step.text,
                missing
                    .iter()
                    .map(|s| format!("`{s}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            evidence: evidence.to_string(),
        };
    }

    StepVerification {
        passed: true,
        reason: String::new(),
        evidence: evidence.to_string(),
    }
}

/// Evaluate a completed plan step's acceptance gate.
///
/// Returns `true` when the step passes (or has no acceptance criteria). A step
/// with `acceptance` text is checked against the `evidence` it reported using
/// [`verify_step`]; quote-wrapped phrases in the acceptance text become
/// required evidence substrings. Intended to be callable from both the
/// `update_plan` tool and unit tests without a full tool round-trip.
#[must_use]
pub fn evaluate_step_gate(step: &PlanStep) -> bool {
    let Some(acc) = &step.acceptance else {
        return true;
    };
    if acc.trim().is_empty() {
        return true;
    }
    let criteria = AcceptanceCriteria {
        criteria: acc.clone(),
        required_substrings: extract_required_substrings(acc),
    };
    let evidence = step.evidence.clone().unwrap_or_default();
    verify_step(step, &criteria, &evidence).passed
}

/// Extract required evidence substrings from an acceptance-criteria string.
///
/// Phrases wrapped in double (`"..."`) or single (`'...'`) quotes are treated
/// as evidence that must appear for the step to pass its gate; free text is
/// kept as the human-readable `criteria` but does not by itself block the
/// step. This lets the model write, e.g.
/// `acceptance: "tests pass" with output containing "test result: ok"`.
fn extract_required_substrings(acceptance: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = acceptance.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let open = bytes[i] as char;
        if open == '"' || open == '\'' {
            let quote = open;
            i += 1;
            let start = i;
            let mut end = i;
            while i < bytes.len() && (bytes[i] as char) != quote {
                end = i + 1;
                i += 1;
            }
            if i < bytes.len() {
                // Closed quote.
                let phrase = acceptance[start..end].trim().to_string();
                if !phrase.is_empty() {
                    out.push(phrase);
                }
            }
            i += 1;
        } else {
            i += 1;
        }
    }
    out
}

fn clean_list(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect()
}

// === UpdatePlanTool - ToolSpec implementation ===

/// Shared reference to `PlanState` for use across tools
pub type SharedPlanState = Arc<Mutex<PlanState>>;

/// Create a new shared `PlanState`
pub fn new_shared_plan_state() -> SharedPlanState {
    Arc::new(Mutex::new(PlanState::default()))
}

/// Create a shared `PlanState` that checkpoints to `path` across process
/// restarts. Any existing checkpoint at `path` is restored on creation, so a
/// restarted process resumes from the last `update_plan` rather than a blank
/// plan.
pub fn new_shared_plan_state_with_persistence(path: PathBuf) -> SharedPlanState {
    let mut state = PlanState::default();
    state.set_persist_path(path);
    state.restore_if_present();
    Arc::new(Mutex::new(state))
}

/// Tool for updating the implementation plan
pub struct UpdatePlanTool {
    plan_state: SharedPlanState,
}

impl UpdatePlanTool {
    pub fn new(plan_state: SharedPlanState) -> Self {
        Self { plan_state }
    }
}

#[async_trait]
impl ToolSpec for UpdatePlanTool {
    fn name(&self) -> &'static str {
        "update_plan"
    }

    fn description(&self) -> &'static str {
        "Update optional high-level strategy metadata for complex initiatives. Use checklist_write for primary Work progress; update_plan should capture phase-level approach changes, not duplicate checklist items. Include sources, critical files, constraints, verification, risks, and handoff context when they help the user review or continue the plan. Each strategy step has a description and status (pending, in_progress, completed)."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "title": {
                    "type": "string",
                    "description": "Optional short title for the plan artifact"
                },
                "objective": {
                    "type": "string",
                    "description": "What the plan is trying to accomplish"
                },
                "context_summary": {
                    "type": "string",
                    "description": "Brief summary of the evidence and current state behind the plan"
                },
                "explanation": {
                    "type": "string",
                    "description": "Legacy-compatible high-level explanation of the plan or approach"
                },
                "sources_used": {
                    "type": "array",
                    "description": "Files, issues, PRs, commands, or other evidence used to ground the plan. Do not include secrets.",
                    "items": { "type": "string" }
                },
                "critical_files": {
                    "type": "array",
                    "description": "Repo paths or surfaces likely to be edited or verified. Do not include secrets.",
                    "items": { "type": "string" }
                },
                "constraints": {
                    "type": "array",
                    "description": "Hard requirements, user preferences, or boundaries the implementation must respect",
                    "items": { "type": "string" }
                },
                "recommended_approach": {
                    "type": "string",
                    "description": "Recommended implementation strategy and important trade-offs"
                },
                "verification_plan": {
                    "type": "string",
                    "description": "Tests, checks, or manual verification expected before the work is considered done"
                },
                "risks_and_unknowns": {
                    "type": "string",
                    "description": "Known risks, blockers, or unresolved questions"
                },
                "handoff_packet": {
                    "type": "string",
                    "description": "Concise continuation notes for another agent or a later session"
                },
                "plan": {
                    "type": "array",
                    "description": "List of plan steps",
                    "items": {
                        "type": "object",
                        "properties": {
                            "step": {
                                "type": "string",
                                "description": "Description of the step"
                            },
                            "status": {
                                "type": "string",
                                "enum": ["pending", "in_progress", "completed"],
                                "description": "Step status"
                            },
                            "depends_on": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "Ids/names of other steps that must complete before this one. Optional."
                            },
                            "scope": {
                                "type": "string",
                                "description": "File/module/system boundary this step owns, to avoid overlapping ownership. Optional."
                            },
                            "role": {
                                "type": "string",
                                "description": "Role this step plays: research, implementation, verification, etc. Optional."
                            },
                            "acceptance": {
                                "type": "string",
                                "description": "Acceptance criteria the step must satisfy to be done. Phrases wrapped in double or single quotes are treated as required evidence substrings by the step gate. Optional."
                            },
                            "evidence": {
                                "type": "string",
                                "description": "Evidence (command output / test result / diff) proving the step is done, supplied when marking it completed. Evaluated against `acceptance`. Optional."
                            }
                        },
                        "required": ["step", "status"]
                    }
                }
            }
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::WritesFiles]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _context: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let empty_plan = Vec::new();
        let plan_items = match input.get("plan") {
            Some(value) => value
                .as_array()
                .ok_or_else(|| ToolError::invalid_input("Invalid 'plan' array"))?,
            None => &empty_plan,
        };

        let mut plan_args = Vec::new();
        for item in plan_items {
            let step = item
                .get("step")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ToolError::invalid_input("Plan item missing 'step'"))?;

            let status_str = item
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("pending");

            let status = StepStatus::from_str(status_str).unwrap_or(StepStatus::Pending);

            plan_args.push(PlanItemArg {
                step: step.to_string(),
                status,
                depends_on: Vec::new(),
                scope: None,
                role: None,
                acceptance: None,
                evidence: None,
            });
        }

        let args = UpdatePlanArgs {
            title: string_field(&input, "title"),
            objective: string_field(&input, "objective"),
            context_summary: string_field(&input, "context_summary"),
            explanation: string_field(&input, "explanation"),
            sources_used: string_vec_field(&input, "sources_used"),
            critical_files: string_vec_field(&input, "critical_files"),
            constraints: string_vec_field(&input, "constraints"),
            recommended_approach: string_field(&input, "recommended_approach"),
            verification_plan: string_field(&input, "verification_plan"),
            risks_and_unknowns: string_field(&input, "risks_and_unknowns"),
            handoff_packet: string_field(&input, "handoff_packet"),
            plan: plan_args,
        };

        let mut state = self.plan_state.lock().await;

        state.update(args);

        let snapshot = state.snapshot();
        let (pending, in_progress, completed) = state.counts();
        let progress = state.progress_percent();

        // Real plan-consistency check: if the user approved a plan, compare the
        // steps we are now tracking against it and surface any deviation from the
        // approved plan so the model (and the user) can see execution has
        // drifted. Steps the model marks completed are gated against their
        // acceptance criteria when present.
        let mut deviation_report = String::new();
        if let Some(approved) = state.approved_plan() {
            let planned = Plan::from_approved(approved);
            let executed = ExecutedSteps {
                steps: state.steps().iter().map(|s| s.text.clone()).collect(),
            };
            let deviations = detect_deviation(&planned, &executed);
            if !deviations.is_empty() {
                deviation_report.push_str("\n\nPlan deviations detected:\n");
                for d in &deviations {
                    deviation_report.push_str(&format!("- {}\n", d.detail));
                }
            }
        }
        // Gate each completed step against its acceptance criteria. A step with
        // no `acceptance` text auto-passes (trust the model, as before); a step
        // with `acceptance` is checked against the evidence it reported when
        // marked completed. Quote-wrapped phrases in the acceptance text become
        // required evidence substrings.
        let mut gate_report = String::new();
        for step in state.steps.iter_mut() {
            if step.status != StepStatus::Completed {
                continue;
            }
            if !evaluate_step_gate(step) {
                // Mark the step so future snapshots/reporting reflect the gate
                // result, but do not revert the Completed status — the work is
                // done, the model just hasn't supplied matching evidence yet.
                step.verification_failed = true;
                if gate_report.is_empty() {
                    gate_report.push_str("\n\nStep gate failures:\n");
                }
                let evidence = step.evidence.clone().unwrap_or_default();
                gate_report.push_str(&format!(
                    "- step `{}` step_gate failed:{} evidence supplied{}\n",
                    step.text,
                    if evidence.trim().is_empty() { " no" } else { "" },
                    if evidence.trim().is_empty() {
                        " — provide `evidence` when marking the step completed"
                    } else {
                        ""
                    }
                ));
            }
        }

        let result = serde_json::to_string_pretty(&snapshot).unwrap_or_else(|_| "{}".to_string());

        Ok(ToolResult::success(format!(
            "Plan updated: {pending} pending, {in_progress} in progress, {completed} completed ({progress}% done)\n{result}{deviation_report}{gate_report}"
        )))
    }
}

#[cfg(test)]
mod plan_deviation_tests {
    use super::*;

    #[test]
    fn detect_deviation_finds_off_plan_step() {
        let planned = Plan::from_approved("1. read config\n2. write tests");
        let mut actual = ExecutedSteps::default();
        actual.record("read config".to_string());
        actual.record("deploy to prod".to_string());
        let deviations = detect_deviation(&planned, &actual);
        assert!(
            deviations.iter().any(|d| d.actual_step == "deploy to prod"),
            "off-plan step must be reported"
        );
        // Approved step never executed is also a deviation.
        assert!(
            deviations
                .iter()
                .any(|d| d.planned_step.as_deref() == Some("write tests")),
            "unexecuted approved step must be reported"
        );
    }

    #[test]
    fn detect_deviation_clean_when_on_plan() {
        let planned = Plan::from_approved("- read config\n- write tests");
        let mut actual = ExecutedSteps::default();
        actual.record("read config".to_string());
        actual.record("write tests".to_string());
        assert!(detect_deviation(&planned, &actual).is_empty());
    }

    #[test]
    fn verify_step_passes_without_criteria() {
        let step = PlanStep::new("do thing".to_string(), StepStatus::Completed);
        let v = verify_step(&step, &AcceptanceCriteria::default(), "");
        assert!(v.passed);
    }

    #[test]
    fn verify_step_step_gate_fails_on_missing_substring() {
        let step = PlanStep::new("run tests".to_string(), StepStatus::Completed);
        let criteria = AcceptanceCriteria {
            criteria: "tests must pass".to_string(),
            required_substrings: vec!["test result: ok".to_string()],
        };
        let v = verify_step(&step, &criteria, "cargo test ... 0 passed");
        assert!(!v.passed);
        assert!(v.reason.contains("step_gate"));
        let ok = verify_step(&step, &criteria, "test result: ok. 5 passed");
        assert!(ok.passed);
    }

    #[test]
    fn update_preserves_structured_fields_roundtrip() {
        let mut state = PlanState::default();
        let args = UpdatePlanArgs {
            plan: vec![PlanItemArg {
                step: "implement parser".to_string(),
                status: StepStatus::Pending,
                depends_on: vec!["design schema".to_string()],
                scope: Some("src/parser.rs".to_string()),
                role: Some("implementation".to_string()),
                acceptance: Some("output contains \"parsed 3 records\"".to_string()),
                evidence: None,
            }],
            ..Default::default()
        };
        state.update(args);

        // snapshot round-trips the new fields
        let snap = state.snapshot();
        let item = &snap.items[0];
        assert_eq!(item.depends_on, vec!["design schema".to_string()]);
        assert_eq!(item.scope.as_deref(), Some("src/parser.rs"));
        assert_eq!(item.role.as_deref(), Some("implementation"));
        assert_eq!(item.acceptance.as_deref(), Some("output contains \"parsed 3 records\""));

        // apply_snapshot rebuilds an equivalent state
        let mut rebuilt = PlanState::default();
        rebuilt.apply_snapshot(snap);
        let rebuilt_item = &rebuilt.steps()[0];
        assert_eq!(rebuilt_item.depends_on, vec!["design schema".to_string()]);
        assert_eq!(rebuilt_item.scope.as_deref(), Some("src/parser.rs"));
        assert_eq!(rebuilt_item.acceptance.as_deref(), Some("output contains \"parsed 3 records\""));
    }

    #[test]
    fn completed_step_with_acceptance_gates_on_evidence() {
        // Missing evidence phrase must fail the gate.
        let mut step = PlanStep::new("run tests".to_string(), StepStatus::Completed);
        step.acceptance = Some("output must contain \"test result: ok\"".to_string());
        step.evidence = Some("cargo test ... 0 passed".to_string());
        assert!(!evaluate_step_gate(&step), "missing evidence phrase must fail the gate");

        // Matching evidence clears the gate failure.
        step.evidence = Some("test result: ok. 5 passed".to_string());
        assert!(evaluate_step_gate(&step), "matching evidence must pass the gate");

        // No acceptance text auto-passes.
        let mut no_criteria = PlanStep::new("write docs".to_string(), StepStatus::Completed);
        no_criteria.evidence = None;
        assert!(evaluate_step_gate(&no_criteria), "absent acceptance must auto-pass");
    }

    #[test]
    fn extract_required_substrings_finds_quoted_phrases() {
        let out = extract_required_substrings("output must contain \"test result: ok\" and '3 passed'");
        assert_eq!(out, vec!["test result: ok".to_string(), "3 passed".to_string()]);
        // Unquoted free text is not treated as a required substring.
        let out2 = extract_required_substrings("tests should pass");
        assert!(out2.is_empty());
    }
}

// === ExitPlanModeTool ===

/// Name of the plan-approval tool, shared with the engine interception path.
pub const EXIT_PLAN_MODE_NAME: &str = "exit_plan_mode";

/// Tool that lets the agent hand a finished plan to the user and ask for
/// approval before implementation starts.
///
/// Plan mode is read-only, so previously the *only* way out was the user
/// manually typing `/exit_plan`. The agent had no way to say "the plan is
/// ready, may I proceed?", and nothing recorded *what* the user agreed to.
/// This tool closes that loop: the engine intercepts it (like
/// `request_user_input`), shows the plan for approval, and on approval
/// switches to Agent mode while stamping the approved snapshot onto the plan
/// state.
///
/// Execution lives in the engine, so [`ToolSpec::execute`] is unreachable and
/// only exists to satisfy the trait.
pub struct ExitPlanModeTool;

#[async_trait]
impl ToolSpec for ExitPlanModeTool {
    fn name(&self) -> &'static str {
        EXIT_PLAN_MODE_NAME
    }

    fn description(&self) -> &'static str {
        "Present the finished implementation plan to the user and ask for approval to start implementing. \
         Only use this in Plan mode, and only after the plan is complete and grounded in the codebase. \
         On approval the session leaves Plan mode and implementation may begin; if the user rejects, \
         revise the plan and ask again. Do not use this for pure research or questions that need no code changes."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "plan": {
                    "type": "string",
                    "description": "The plan to present for approval, as concise markdown. Should cover what will change and how it will be verified."
                }
            },
            "required": ["plan"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    async fn execute(
        &self,
        _input: serde_json::Value,
        _context: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        Err(ToolError::execution_failed(
            "exit_plan_mode must be handled by the engine",
        ))
    }
}

/// Extract and validate the `plan` argument of an `exit_plan_mode` call.
pub fn exit_plan_mode_plan_text(input: &serde_json::Value) -> Result<String, ToolError> {
    let plan = input
        .get("plan")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .unwrap_or("");
    if plan.is_empty() {
        return Err(ToolError::invalid_input(
            "exit_plan_mode requires a non-empty 'plan'",
        ));
    }
    Ok(plan.to_string())
}

fn string_field(input: &serde_json::Value, field: &str) -> Option<String> {
    input
        .get(field)
        .and_then(|v| v.as_str())
        .map(std::string::ToString::to_string)
}

fn string_vec_field(input: &serde_json::Value, field: &str) -> Vec<String> {
    input
        .get(field)
        .and_then(|v| v.as_array())
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str().map(std::string::ToString::to_string))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod plan_persist_tests {
    use super::*;

    #[test]
    fn plan_state_apply_snapshot_restores_fields_and_steps() {
        let mut state = PlanState::default();
        state.update(UpdatePlanArgs {
            title: Some("My Plan".to_string()),
            objective: Some("Ship it".to_string()),
            plan: vec![
                PlanItemArg {
                    step: "first".to_string(),
                    status: StepStatus::Completed,
                    ..Default::default()
                },
                PlanItemArg {
                    step: "second".to_string(),
                    status: StepStatus::InProgress,
                    ..Default::default()
                },
            ],
            ..Default::default()
        });

        let snap = state.snapshot();
        // Serialize + deserialize to prove Deserialize is wired up on PlanSnapshot.
        let json = serde_json::to_string(&snap).unwrap();
        let snap: PlanSnapshot = serde_json::from_str(&json).unwrap();

        let mut restored = PlanState::default();
        restored.apply_snapshot(snap);

        assert_eq!(restored.snapshot().title.as_deref(), Some("My Plan"));
        assert_eq!(restored.snapshot().objective.as_deref(), Some("Ship it"));
        assert_eq!(restored.steps().len(), 2);
        assert_eq!(restored.steps()[0].status, StepStatus::Completed);
        assert_eq!(restored.steps()[1].status, StepStatus::InProgress);
    }

    #[test]
    fn plan_snapshot_empty_steps_are_dropped_on_apply() {
        let snap = PlanSnapshot {
            items: vec![PlanItemArg {
                step: "   ".to_string(),
                status: StepStatus::Pending,
                ..Default::default()
            }],
            ..Default::default()
        };
        let mut state = PlanState::default();
        state.apply_snapshot(snap);
        assert!(state.steps().is_empty());
    }

    #[test]
    fn exit_plan_mode_plan_text_extracts_and_trims() {
        let input = json!({ "plan": "  Step 1: do the thing  " });
        assert_eq!(
            exit_plan_mode_plan_text(&input).unwrap(),
            "Step 1: do the thing"
        );
    }

    #[test]
    fn exit_plan_mode_plan_text_rejects_missing_or_blank() {
        // A blank plan would drop the user into Agent mode with nothing to
        // review, which defeats the point of the approval gate.
        assert!(exit_plan_mode_plan_text(&json!({})).is_err());
        assert!(exit_plan_mode_plan_text(&json!({ "plan": "" })).is_err());
        assert!(exit_plan_mode_plan_text(&json!({ "plan": "   " })).is_err());
        assert!(exit_plan_mode_plan_text(&json!({ "plan": 42 })).is_err());
    }

    #[test]
    fn plan_state_records_approved_plan() {
        let mut state = PlanState::default();
        assert_eq!(state.approved_plan(), None);
        state.set_approved_plan("ship the feature".to_string());
        assert_eq!(state.approved_plan(), Some("ship the feature"));
    }

    #[test]
    fn exit_plan_mode_tool_requires_plan_argument() {
        let tool = ExitPlanModeTool;
        assert_eq!(tool.name(), EXIT_PLAN_MODE_NAME);
        let schema = tool.input_schema();
        assert_eq!(schema["required"], json!(["plan"]));
    }
}
