//! Free helper functions for the subagent domain.
//!
//! Extracted from `subagent/mod.rs`. Accesses parent-module items via
//! `use super::*` and is re-exported with `pub(crate) use helpers::*`.

use super::*;

pub(crate) fn release_resident_leases_for(agent_id: &str) {
    if let Some(lock) = RESIDENT_LEASES.get() {
        match lock.lock() {
            Ok(mut guard) => {
                guard.retain(|_, owner| owner != agent_id);
            }
            Err(poisoned) => {
                tracing::warn!(
                    target: "subagent",
                    agent_id,
                    ?poisoned,
                    "RESIDENT_LEASES mutex poisoned; lease cleanup skipped"
                );
                // Recover from poison to avoid permanent lock.
                let mut guard = poisoned.into_inner();
                guard.retain(|_, owner| owner != agent_id);
            }
        }
    }
}

/// Default maximum steps for sub-agent loops. Set to `u32::MAX` to remove the
/// arbitrary fixed cap (#2034). Sub-agents run until they produce a final text
/// response (no tool calls), are cancelled by the parent, or hit a configured
/// explicit budget. Callers that want a hard bound can override `max_steps` on
/// the `SubAgentManager`.
pub(crate) const DEFAULT_MAX_STEPS: u32 = u32::MAX;
/// Default wall-clock budget for a single sub-agent tool execution. The active
/// value travels on `SubAgentRuntime::tool_timeout` so a long-but-legitimate
/// tool (a large build, a slow shell command, a deep search) is not killed
/// mid-flight. Kept non-zero so `timeout(Duration::ZERO, ...)` can never fire
/// immediately. The per-step API timeout, streaming watchdogs, and heartbeat
/// floors remain the independent stall detectors.
pub(crate) const DEFAULT_TOOL_TIMEOUT: Duration = Duration::from_secs(300);
pub(crate) const MIN_SUBAGENT_SPAWN_TOKEN_RESERVE: u64 = 1;

/// Format a step counter for sub-agent progress messages.
///
/// When `max_steps == u32::MAX` (the default), the denominator is a sentinel
/// meaning "unbounded" — render just `step N` instead of `step N/4294967295`.
pub(crate) fn format_step_counter(steps: u32, max_steps: u32) -> String {
    if max_steps == u32::MAX {
        format!("step {steps}")
    } else {
        format!("step {steps}/{max_steps}")
    }
}
// Non-streaming sub-agents need enough response budget to carry large tool-call
// arguments, especially write_file content. The API bills generated tokens, not
// the requested ceiling.
pub(crate) const SUBAGENT_RESPONSE_MAX_TOKENS: u32 = 16_384;
pub(crate) const MAX_CONSECUTIVE_TRUNCATED_SUBAGENT_RESPONSES: u32 = 5;
pub(crate) const SUBAGENT_TRANSIENT_PROVIDER_MAX_RETRIES: u32 = 2;
pub(crate) const SUBAGENT_TRANSIENT_PROVIDER_INITIAL_BACKOFF: Duration = Duration::from_millis(250);
/// Per-step LLM API call timeout. Each `create_message` request must complete
/// within this window or the step is treated as timed out. Prevents a single
/// stuck API call from blocking the sub-agent indefinitely.
/// Legacy fallback for the per-step DeepSeek API timeout. The active timeout
/// now travels on `SubAgentRuntime::step_api_timeout` so users can override
/// it via `[subagents] api_timeout_secs` in `~/.mimofan/config.toml`. The
/// constant only exists for tests/stub runtimes that need a hard-coded
/// default; production runtimes set the field explicitly (#1806, #1808).
pub(crate) const DEFAULT_STEP_API_TIMEOUT: Duration =
    Duration::from_secs(crate::config::DEFAULT_SUBAGENT_API_TIMEOUT_SECS);
pub(crate) const COMPLETED_AGENT_RETENTION: Duration = Duration::from_secs(60 * 60);
pub(crate) const MAX_AGENT_WORKER_RECORDS: usize = 256;
pub(crate) const MAX_AGENT_WORKER_EVENTS_PER_RECORD: usize = 128;
pub(crate) const SUBAGENT_STATE_SCHEMA_VERSION: u32 = 1;
pub(crate) const SUBAGENT_STATE_FILE: &str = "subagents.v1.json";
pub(crate) const SUBAGENT_WORKTREE_ROOT_DIR: &str = ".mimofan-worktrees";
pub(crate) const SUBAGENT_RESTART_REASON: &str = "Interrupted by process restart";
pub(crate) const SUBAGENT_QUEUED_LAUNCH_REASON: &str =
    "queued: waiting for a sub-agent launch slot";
pub(crate) const SUBAGENT_MODEL_WAIT_REASON: &str = "waiting for model response";
/// #freeze: minimum spacing between hot-path (per-step checkpoint) state
/// persists. `update_checkpoint` fires on every step of every agent; at high
/// fanout an unconditional full-fleet rewrite under the manager write lock
/// wedges the UI. Hot-path writes coalesce to at most one per this interval;
/// terminal/structural changes still persist immediately, and any terminal
/// write flushes the full in-memory fleet (including other agents' pending
/// checkpoints) to disk.
pub(crate) const SUBAGENT_PERSIST_DEBOUNCE: Duration = Duration::from_millis(1500);

/// #freeze: lightweight perf counters for the sub-agent persist hot path,
/// gated behind `MIMOFAN_SUBAGENT_PERF_TRACE=1`. The atomic increments are
/// always cheap; only the structured `subagent_perf` log line is gated.
pub(crate) static SUBAGENT_PERSIST_WRITES: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);
pub(crate) static SUBAGENT_PERSIST_SKIPPED: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

pub(crate) fn subagent_perf_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var("MIMOFAN_SUBAGENT_PERF_TRACE")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
    })
}

pub(crate) const VALID_SUBAGENT_TYPES: &str = "general (aliases: general-purpose, general_purpose, worker, default), \
     explore (aliases: exploration, explorer), plan (aliases: planning, planner, awaiter), \
     review (aliases: code-review, code_review, reviewer), implementer (aliases: implement, implementation, builder), \
     verifier (aliases: verify, verification, validator, tester), custom";
/// Role aliases accepted by `normalize_role_alias`. Kept in sync with the
/// match arms below so every input that `SubAgentType::from_str` accepts also
/// resolves to a canonical role (avoids the dual-validation rejection in #2649).
pub(crate) const VALID_ROLE_ALIASES: &str = "default; worker (aliases: general, general-purpose, general_purpose); \
     explorer (aliases: explore, exploration); awaiter (aliases: plan, planning, planner); \
     reviewer (aliases: review, code-review, code_review); implementer (aliases: implement, implementation, builder); \
     verifier (aliases: verify, verification, validator, tester); custom";
pub(crate) const SUBAGENT_TYPE_DESCRIPTION: &str = "Sub-agent type. Accepted vocabulary: general (aliases: general-purpose, general_purpose, worker, default), \
     explore (aliases: exploration, explorer), plan (aliases: planning, planner, awaiter), \
     review (aliases: code-review, code_review, reviewer), implementer (aliases: implement, implementation, builder), \
     verifier (aliases: verify, verification, validator, tester), custom.";
/// Mimofan species used as friendly names for sub-agents in the UI. The full
/// Cetacea infraorder — baleen whales (Mysticeti), toothed whales
/// (Odontoceti), plus select dolphin species (family Delphinidae) that
/// don't conflate with existing agent type labels. Porpoises (Phocoenidae)
/// are excluded because their name doesn't carry well as a friendly label.
///
/// English and Simplified-Chinese names are interleaved so any newly spawned
/// agent has a roughly even chance of either — the goal is friendly variety,
/// not a strict locale match.
///
/// Taxonomy source: Society for Marine Mammalogy (2025).
pub const MIMOFAN_NICKNAMES: &[&str] = &[
    "Blue",
    "蓝鲸",
    "Humpback",
    "座头鲸",
    "Sperm",
    "抹香鲸",
    "Fin",
    "长须鲸",
    "Sei",
    "塞鲸",
    "Bryde's",
    "布氏鲸",
    "Minke",
    "小须鲸",
    "Antarctic Minke",
    "南极小须鲸",
    "Pygmy Right",
    "小露脊鲸",
    "Omura's",
    "大村鲸",
    "Eden's",
    "艾氏鲸",
    "Rice's",
    "赖斯鲸",
    "Gray",
    "灰鲸",
    "Bowhead",
    "弓头鲸",
    "North Atlantic Right",
    "北大西洋露脊鲸",
    "North Pacific Right",
    "北太平洋露脊鲸",
    "Southern Right",
    "南露脊鲸",
    "Beluga",
    "白鲸",
    "Narwhal",
    "独角鲸",
    "Orca",
    "虎鲸",
    "Pilot",
    "领航鲸",
    "False Killer",
    "伪虎鲸",
    "Pygmy Killer",
    "小虎鲸",
    "Melon-headed",
    "瓜头鲸",
    "Beaked",
    "喙鲸",
    "Cuvier's Beaked",
    "柯氏喙鲸",
    "Baird's Beaked",
    "贝氏喙鲸",
    "Blainville's Beaked",
    "柏氏喙鲸",
    "Ginkgo-toothed Beaked",
    "银杏齿喙鲸",
    "Strap-toothed",
    "带齿喙鲸",
    "Stejneger's Beaked",
    "斯氏喙鲸",
    "Dwarf Sperm",
    "小抹香鲸",
    "Pygmy Sperm",
    "侏儒抹香鲸",
    "Rough-toothed",
    "糙齿海豚",
    "Atlantic Spotted",
    "大西洋斑海豚",
    "Pantropical Spotted",
    "热带斑海豚",
    "Spinner",
    "长吻飞旋海豚",
    "Clymene",
    "短吻飞旋海豚",
    "Striped",
    "条纹海豚",
    "Common Bottlenose",
    "宽吻海豚",
    "Indo-Pacific Bottlenose",
    "印太瓶鼻海豚",
    "Risso's",
    "灰海豚",
    "Commerson's",
    "花斑海豚",
    "Chilean",
    "智利海豚",
    "Heaviside's",
    "海氏矮海豚",
    "Hector's",
    "赫氏矮海豚",
    "Amazon River",
    "亚马逊河豚",
    "Ganges River",
    "恒河豚",
    "Indus River",
    "印度河豚",
    "La Plata",
    "拉普拉塔河豚",
    "Franciscana",
    "拉河豚",
];

// === Types ===

pub(crate) fn default_subagent_actor_kind() -> String {
    "subagent".to_string()
}

pub(crate) fn default_agent_inspect_tool() -> String {
    "handle_read".to_string()
}

pub(crate) fn default_subagent_takeover_kind() -> String {
    "local_subagent_session".to_string()
}

pub(crate) fn default_agent_run_follow_up() -> AgentRunFollowUpTarget {
    AgentRunFollowUpTarget {
        tool: default_agent_inspect_tool(),
        agent_id: String::new(),
        session_name: None,
        accepted_statuses: vec!["running".to_string(), "interrupted_continuable".to_string()],
        latest_delivery: None,
    }
}

pub(crate) fn default_agent_run_takeover() -> AgentRunTakeoverTarget {
    AgentRunTakeoverTarget {
        kind: default_subagent_takeover_kind(),
        supported: false,
        agent_id: String::new(),
        session_name: None,
        instructions: "No takeover target is available for this older record.".to_string(),
        unsupported_reason: Some("legacy_record_missing_agent_id".to_string()),
    }
}

pub(crate) fn default_agent_run_usage() -> AgentRunUsage {
    AgentRunUsage {
        status: "unknown".to_string(),
        input_tokens: None,
        output_tokens: None,
        total_tokens: None,
        token_budget: None,
        budget_spent_tokens: None,
        budget_remaining_tokens: None,
        budget_scope: None,
        note: "Token usage is not yet reported by the sub-agent worker ledger.".to_string(),
    }
}

pub(crate) fn positive_token_budget(budget: Option<u64>) -> Option<u64> {
    budget.filter(|value| *value > 0)
}

pub(crate) fn usage_total_tokens(usage: &Usage) -> u64 {
    u64::from(usage.input_tokens).saturating_add(u64::from(usage.output_tokens))
}

pub(crate) fn refresh_usage_note(usage: &mut AgentRunUsage) {
    let worker_total = usage.total_tokens.unwrap_or(0);
    if let Some(limit) = usage.token_budget {
        let spent = usage.budget_spent_tokens.unwrap_or(worker_total);
        let remaining = usage
            .budget_remaining_tokens
            .unwrap_or_else(|| limit.saturating_sub(spent));
        usage.status = if remaining == 0 {
            "budget_exhausted".to_string()
        } else if worker_total > 0 {
            "reported".to_string()
        } else {
            "tracking".to_string()
        };
        usage.note = if worker_total > 0 {
            format!(
                "Token budget: {spent}/{limit} spent, {remaining} remaining. This worker reported {worker_total} tokens."
            )
        } else {
            format!("Token budget: {spent}/{limit} spent, {remaining} remaining.")
        };
    } else if worker_total > 0 {
        usage.status = "reported".to_string();
        usage.note = format!("Provider reported {worker_total} tokens for this worker.");
    } else if usage.status.is_empty() {
        *usage = default_agent_run_usage();
    }
}

pub(crate) fn default_agent_run_verification() -> AgentRunVerificationSummary {
    AgentRunVerificationSummary {
        status: "self_report_only".to_string(),
        summary:
            "No verified command or test receipt is attached; treat the result summary as a child self-report."
                .to_string(),
    }
}

pub(crate) fn default_agent_run_recommended_action() -> AgentRunRecommendedAction {
    AgentRunRecommendedAction {
        action: "inspect_transcript".to_string(),
        tool: Some(default_agent_inspect_tool()),
        reason: "Inspect the returned transcript handle if the child result needs audit detail."
            .to_string(),
    }
}

pub(crate) fn recommended_action_for_worker_status(
    status: AgentWorkerStatus,
    spec: &AgentWorkerSpec,
) -> AgentRunRecommendedAction {
    let agent_ref = spec
        .session_name
        .as_deref()
        .filter(|name| !name.is_empty())
        .unwrap_or(&spec.worker_id);
    match status {
        AgentWorkerStatus::Queued => AgentRunRecommendedAction {
            action: "continue_parent_work".to_string(),
            tool: None,
            reason: format!(
                "Worker {agent_ref} is queued in the background; continue coordinating and consume its completion event when it arrives."
            ),
        },
        AgentWorkerStatus::Starting
        | AgentWorkerStatus::Running
        | AgentWorkerStatus::ModelWait
        | AgentWorkerStatus::RunningTool => AgentRunRecommendedAction {
            action: "continue_parent_work".to_string(),
            tool: None,
            reason: format!(
                "Worker {agent_ref} is active in the background; continue parent work until its completion event arrives."
            ),
        },
        AgentWorkerStatus::WaitingForUser => AgentRunRecommendedAction {
            action: "inspect_or_replace".to_string(),
            tool: Some(default_agent_inspect_tool()),
            reason: format!(
                "Worker {agent_ref} needs parent action; inspect the transcript handle and open a replacement with agent if the task still matters."
            ),
        },
        AgentWorkerStatus::Completed => AgentRunRecommendedAction {
            action: "verify_self_report".to_string(),
            tool: Some("handle_read".to_string()),
            reason: format!(
                "Worker {agent_ref} completed; verify its self-report before treating side effects as fact."
            ),
        },
        AgentWorkerStatus::Failed => AgentRunRecommendedAction {
            action: "inspect_failure".to_string(),
            tool: Some(default_agent_inspect_tool()),
            reason: format!(
                "Worker {agent_ref} failed; inspect the transcript handle and decide whether to open a replacement."
            ),
        },
        AgentWorkerStatus::Cancelled => AgentRunRecommendedAction {
            action: "open_replacement_if_needed".to_string(),
            tool: Some("agent".to_string()),
            reason: format!(
                "Worker {agent_ref} was cancelled; open a replacement with agent only if the assignment still matters."
            ),
        },
        AgentWorkerStatus::Interrupted => AgentRunRecommendedAction {
            action: "inspect_or_replace".to_string(),
            tool: Some(default_agent_inspect_tool()),
            reason: format!(
                "Worker {agent_ref} was interrupted; inspect the transcript handle before deciding whether to re-dispatch."
            ),
        },
    }
}

pub(crate) fn agent_worker_run_id(spec: &AgentWorkerSpec) -> String {
    if spec.run_id.is_empty() {
        spec.worker_id.clone()
    } else {
        spec.run_id.clone()
    }
}

pub(crate) fn follow_up_target_for_spec(spec: &AgentWorkerSpec) -> AgentRunFollowUpTarget {
    AgentRunFollowUpTarget {
        tool: default_agent_inspect_tool(),
        agent_id: spec.worker_id.clone(),
        session_name: spec.session_name.clone(),
        accepted_statuses: vec!["running".to_string(), "interrupted_continuable".to_string()],
        latest_delivery: None,
    }
}

pub(crate) fn takeover_target_for_spec(spec: &AgentWorkerSpec) -> AgentRunTakeoverTarget {
    let agent_ref = spec
        .session_name
        .as_deref()
        .filter(|name| !name.is_empty())
        .unwrap_or(&spec.worker_id);
    AgentRunTakeoverTarget {
        kind: default_subagent_takeover_kind(),
        supported: true,
        agent_id: spec.worker_id.clone(),
        session_name: spec.session_name.clone(),
        instructions: format!(
            "Inspect agent '{agent_ref}' through the returned transcript_handle with handle_read; open a replacement with agent if the lane no longer fits."
        ),
        unsupported_reason: None,
    }
}

pub(crate) fn default_subagent_artifacts(run_id: &str) -> Vec<AgentRunArtifactRef> {
    vec![
        AgentRunArtifactRef {
            kind: "worker_events".to_string(),
            name: "worker_record.events".to_string(),
            target: run_id.to_string(),
            description: "Bounded structured lifecycle events retained on the worker record."
                .to_string(),
        },
        AgentRunArtifactRef {
            kind: "transcript".to_string(),
            name: "transcript_handle".to_string(),
            target: format!("agent:{run_id}"),
            description:
                "Use the projection transcript_handle with handle_read for the child transcript."
                    .to_string(),
        },
        AgentRunArtifactRef {
            kind: "receipt".to_string(),
            name: "result_summary".to_string(),
            target: run_id.to_string(),
            description: "Child final summary when present; verify before treating as fact."
                .to_string(),
        },
    ]
}

pub(crate) fn normalize_worker_spec(mut spec: AgentWorkerSpec) -> AgentWorkerSpec {
    if spec.run_id.is_empty() {
        spec.run_id = spec.worker_id.clone();
    }
    spec
}

pub(crate) fn worker_tool_scope(tool_profile: &AgentWorkerToolProfile) -> ToolScope {
    match tool_profile {
        AgentWorkerToolProfile::Inherited => ToolScope::Inherit,
        AgentWorkerToolProfile::Explicit(tools) => ToolScope::Explicit(tools.clone()),
    }
}

pub(crate) fn worker_profile_from_spec(spec: &AgentWorkerSpec) -> WorkerRuntimeProfile {
    let mut profile = WorkerRuntimeProfile::for_role(spec.agent_type.clone());
    profile.tools = worker_tool_scope(&spec.tool_profile);
    profile.model = ModelRoute::Fixed(spec.model.clone());
    profile.max_spawn_depth = spec.max_spawn_depth.saturating_sub(spec.spawn_depth);
    profile.background = true;
    profile
}

pub(crate) fn worker_profile_for_spawn(
    runtime: &SubAgentRuntime,
    agent_type: &SubAgentType,
    tool_profile: &AgentWorkerToolProfile,
    effective_model: &str,
    model_route: Option<ModelRoute>,
) -> WorkerRuntimeProfile {
    let mut requested = WorkerRuntimeProfile::for_role(agent_type.clone());
    requested.tools = worker_tool_scope(tool_profile);
    requested.model = model_route.unwrap_or_else(|| ModelRoute::Fixed(effective_model.to_string()));
    requested.provider = Some(runtime.client.api_provider().as_str().to_string());
    requested.max_spawn_depth = runtime.max_spawn_depth.saturating_sub(runtime.spawn_depth);
    requested.background = true;
    runtime.worker_profile.derive_child(&requested)
}

pub(crate) fn normalize_worker_record(mut record: AgentWorkerRecord) -> AgentWorkerRecord {
    record.spec = normalize_worker_spec(record.spec);
    if record.spec.runtime_profile == WorkerRuntimeProfile::default() {
        record.spec.runtime_profile = worker_profile_from_spec(&record.spec);
    }
    let run_id = agent_worker_run_id(&record.spec);
    if record.actor_kind.is_empty() {
        record.actor_kind = default_subagent_actor_kind();
    }
    if record.parent_run_id.is_none() {
        record.parent_run_id = record.spec.parent_run_id.clone();
    }
    if record.follow_up.agent_id.is_empty() {
        record.follow_up = follow_up_target_for_spec(&record.spec);
    } else if record.follow_up.tool != default_agent_inspect_tool() {
        record.follow_up.tool = default_agent_inspect_tool();
    }
    if record.takeover.agent_id.is_empty()
        || !record
            .takeover
            .instructions
            .contains(&default_agent_inspect_tool())
    {
        record.takeover = takeover_target_for_spec(&record.spec);
    }
    record.recommended_action =
        recommended_action_for_worker_status(record.status.clone(), &record.spec);
    if record.artifacts.is_empty() {
        record.artifacts = default_subagent_artifacts(&run_id);
    }
    if record.usage.status.is_empty() {
        record.usage = default_agent_run_usage();
    } else {
        refresh_usage_note(&mut record.usage);
    }
    if record.verification.status.is_empty() {
        record.verification = default_agent_run_verification();
    }
    record
}

pub(crate) fn current_git_branch(workspace: &Path) -> Option<String> {
    let branch = run_git(workspace, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    let branch = branch.trim();
    if branch.is_empty() {
        return None;
    }
    if branch != "HEAD" {
        return Some(branch.to_string());
    }

    let short_hash = run_git(workspace, &["rev-parse", "--short", "HEAD"])?;
    let short_hash = short_hash.trim();
    (!short_hash.is_empty()).then(|| format!("detached:{short_hash}"))
}

pub(crate) fn run_git(workspace: &Path, args: &[&str]) -> Option<String> {
    let output = Git::output(args, workspace).ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).to_string())
}

/// Best-effort removal of an isolated sub-agent git worktree (#691).
///
/// Resolves the owning repository root from the worktree path, then runs
/// `git worktree remove --force <path>`. Failures are logged and swallowed:
/// worktree cleanup must never abort the parent turn or panic on a missing /
/// already-removed checkout. The branch created alongside the worktree is left
/// in place (git refuses to remove a worktree whose branch is checked out
/// elsewhere, and keeping it is harmless and useful for debugging).
pub(crate) fn remove_worktree(worktree_path: &Path) {
    let root = match run_git(worktree_path, &["rev-parse", "--show-toplevel"]) {
        Some(root) if !root.trim().is_empty() => PathBuf::from(root.trim()),
        _ => {
            tracing::warn!(
                target: "subagent",
                path = %worktree_path.display(),
                "could not resolve git root for worktree cleanup; skipping"
            );
            return;
        }
    };
    match run_git(&root, &["worktree", "remove", "--force", &worktree_path.to_string_lossy()]) {
        Some(_) => {
            tracing::debug!(
                target: "subagent",
                path = %worktree_path.display(),
                "removed isolated sub-agent worktree"
            );
        }
        None => {
            tracing::warn!(
                target: "subagent",
                path = %worktree_path.display(),
                "git worktree remove failed; leaving checkout for manual cleanup"
            );
        }
    }
}

/// Reclaim orphaned worktree metadata left by crashed / force-killed
/// sub-agents. Safe to call on every spawn; `git worktree prune` only removes
/// entries whose administrative directory is missing (#691).
pub(crate) fn prune_orphan_worktrees(repo_workspace: &Path) {
    if let Some(root) = run_git(repo_workspace, &["rev-parse", "--show-toplevel"]) {
        let root = root.trim();
        if !root.is_empty() {
            // Fire-and-forget: prune failures are non-fatal.
            let _ = run_git(Path::new(root), &["worktree", "prune"]);
        }
    }
}
