//! Fleet worker runtime — bridges fleet task specs to headless sub-agent execution.
//!
//! This module makes fleet workers real: instead of simulating task completion,
//! each fleet worker spawns a headless sub-agent that runs the task instructions
//! and streams progress back into the fleet ledger.
//!
//! Architecture:
//! - `FleetTaskSpec` + `FleetWorkerSpec` → `AgentWorkerSpec`
//! - `SubAgentManager::register_worker()` tracks the worker
//! - Sub-agent spawn happens through the existing `agent` machinery
//! - Mailbox events stream into fleet ledger as `FleetWorkerEventPayload`
//! - `FleetWorkerInspection` reads both ledger state and sub-agent worker records

#![allow(dead_code)]

use anyhow::{Result, bail};
use mimofan_protocol::fleet::{
    FleetHostSpec, FleetResolvedRoute, FleetTaskSpec, FleetTaskWorkerProfile,
    FleetWorkerEventPayload, FleetWorkerSpec,
};

use super::host::FleetHostKind;
use super::profile::AgentProfile;
use crate::config::ApiProvider;
use crate::route_runtime::resolve_route_candidate;
use crate::tools::subagent::{
    AgentWorkerSpec, AgentWorkerStatus, AgentWorkerToolProfile, SubAgentType,
};
use crate::worker_profile::{ModelRoute, ToolScope, WorkerRuntimeProfile};

/// Map a fleet worker spec's host kind to a display string.
pub fn fleet_host_kind_for_spec(spec: &FleetWorkerSpec) -> FleetHostKind {
    match &spec.host {
        FleetHostSpec::Local => FleetHostKind::LocalProcess,
        FleetHostSpec::Ssh { .. } => FleetHostKind::Ssh,
        FleetHostSpec::Docker { .. } => FleetHostKind::LocalProcess, // Docker runs local-ish
    }
}

/// Map a fleet host kind to a compact display label.
pub fn fleet_host_kind_label(kind: FleetHostKind) -> &'static str {
    match kind {
        FleetHostKind::LocalProcess => "local",
        FleetHostKind::Ssh => "ssh",
    }
}

/// Build a sub-agent `AgentWorkerSpec` from a fleet task spec and worker spec.
///
/// The fleet task's `instructions` become the sub-agent's `objective`, the
/// `worker.role` maps to a `SubAgentType`, and tool/capability restrictions
/// become an `AgentWorkerToolProfile`.
pub fn fleet_task_to_worker_spec(
    worker_id: &str,
    run_id: &str,
    task_spec: &FleetTaskSpec,
    _worker_spec: &FleetWorkerSpec,
    model: &str,
    workspace: &std::path::Path,
) -> AgentWorkerSpec {
    let agent_type =
        fleet_role_to_agent_type(task_spec.worker.as_ref().and_then(|w| w.role.as_deref()));

    let tool_profile = fleet_tool_profile(task_spec.worker.as_ref());

    let objective = fleet_task_prompt(task_spec);
    let max_spawn_depth = mimofan_config::FleetExecConfig::default().max_spawn_depth;
    let runtime_profile =
        fleet_worker_runtime_profile(&agent_type, &tool_profile, model, 0, max_spawn_depth);

    AgentWorkerSpec {
        worker_id: worker_id.to_string(),
        run_id: run_id.to_string(),
        parent_run_id: None,
        session_name: Some(format!("fleet-{}-{}", worker_id, task_spec.id)),
        objective,
        role: task_spec.worker.as_ref().and_then(|w| w.role.clone()),
        agent_type,
        model: model.to_string(),
        workspace: workspace.to_path_buf(),
        git_branch: None,
        context_mode: "fresh".to_string(),
        fork_context: false,
        tool_profile,
        runtime_profile,
        max_steps: task_spec
            .budget
            .as_ref()
            .and_then(|b| b.max_tool_calls)
            .unwrap_or(u32::MAX),
        spawn_depth: 0,
        max_spawn_depth,
    }
}

/// Validate that every task referencing a workspace agent profile can resolve it.
///
/// This is intended to run at Fleet run creation time, before leasing any
/// worker or appending lifecycle events.
pub fn validate_task_agent_profiles(
    tasks: &[FleetTaskSpec],
    agent_profiles: &[AgentProfile],
) -> Result<()> {
    for task in tasks {
        resolve_task_agent_profile(task, agent_profiles)?;
    }
    Ok(())
}

/// Build a sub-agent worker spec after resolving workspace Fleet profile input.
///
/// This keeps Fleet and sub-agents on the same runtime substrate: profile files
/// and task-level role/loadout intent are composed into the existing
/// `AgentWorkerSpec` / `WorkerRuntimeProfile` pair, then optionally intersected
/// with a parent profile when the caller has one.
#[allow(clippy::too_many_arguments)]
pub fn fleet_task_to_worker_spec_with_profiles(
    worker_id: &str,
    run_id: &str,
    task_spec: &FleetTaskSpec,
    _worker_spec: &FleetWorkerSpec,
    model: &str,
    workspace: &std::path::Path,
    agent_profiles: &[AgentProfile],
    parent_runtime_profile: Option<&WorkerRuntimeProfile>,
) -> Result<AgentWorkerSpec> {
    let agent_profile = resolve_task_agent_profile(task_spec, agent_profiles)?;
    let worker_profile = task_spec.worker.as_ref();
    let role = effective_fleet_role(worker_profile, agent_profile);
    let agent_type = fleet_role_to_agent_type(role.as_deref());
    let tool_profile = fleet_tool_profile(worker_profile);
    let objective = fleet_task_prompt_with_profile(task_spec, agent_profile);
    let max_spawn_depth = mimofan_config::FleetExecConfig::default().max_spawn_depth;
    let loadout = effective_fleet_loadout(worker_profile, agent_profile);
    let effective_model = effective_fleet_model(model, worker_profile, agent_profile);
    let mut requested_runtime = fleet_worker_runtime_profile_for_loadout(
        &agent_type,
        &tool_profile,
        &effective_model,
        0,
        max_spawn_depth,
        &loadout,
    );
    if let Some(agent_profile) = agent_profile
        && let Some(profile_depth) = agent_profile.profile.delegation.max_spawn_depth
    {
        requested_runtime.max_spawn_depth = requested_runtime.max_spawn_depth.min(profile_depth);
    }
    let runtime_profile = parent_runtime_profile
        .map(|parent| parent.derive_child(&requested_runtime))
        .unwrap_or(requested_runtime);

    Ok(AgentWorkerSpec {
        worker_id: worker_id.to_string(),
        run_id: run_id.to_string(),
        parent_run_id: None,
        session_name: Some(format!("fleet-{}-{}", worker_id, task_spec.id)),
        objective,
        role,
        agent_type,
        model: effective_model,
        workspace: workspace.to_path_buf(),
        git_branch: None,
        context_mode: "fresh".to_string(),
        fork_context: false,
        tool_profile,
        runtime_profile: runtime_profile.clone(),
        max_steps: task_spec
            .budget
            .as_ref()
            .and_then(|b| b.max_tool_calls)
            .unwrap_or(u32::MAX),
        spawn_depth: 0,
        max_spawn_depth: runtime_profile.max_spawn_depth,
    })
}

/// Mint a [`FleetResolvedRoute`] snapshot for a fleet task (#3154).
///
/// This calls the existing hermetic resolver bridge
/// ([`resolve_route_candidate`]) so the persisted route reflects the same
/// resolution semantics the runtime would use, then records only non-sensitive
/// shape (provider id/kind, model ids, protocol) combined with the already
/// computed effective role/loadout intent. `source` is `"resolver"`.
///
/// Honesty rules:
/// - `canonical_model` stays `None` when the resolver could not pin one.
/// - The provider comes from the resolver default (the worker profile carries
///   no provider authority); a task-level `model` selector is forwarded as the
///   model selector. No reasoning/pricing fields are fabricated.
///
/// Returns `None` (never a fabricated route) when resolution fails, so callers
/// degrade gracefully without inventing detail.
pub(crate) fn resolve_fleet_route(
    task_spec: &FleetTaskSpec,
    agent_profiles: &[AgentProfile],
) -> Option<FleetResolvedRoute> {
    let agent_profile = resolve_task_agent_profile(task_spec, agent_profiles)
        .ok()
        .flatten();
    let worker_profile = task_spec.worker.as_ref();
    let role = effective_fleet_role(worker_profile, agent_profile);
    let loadout = effective_fleet_loadout(worker_profile, agent_profile);

    // A task-level explicit model is the only model selector the spec carries
    // with provider-resolution intent; otherwise let the resolver pick the
    // provider default. Provider authority belongs to route resolution, so we
    // do not infer a provider here.
    let model_selector = worker_profile
        .and_then(|worker| worker.model.as_deref())
        .map(str::trim)
        .filter(|model| !model.is_empty() && *model != "auto");

    // The worker profile carries no provider authority, so resolve within the
    // default provider scope (mirrors `ProviderKind::default()`). The resolver
    // is fully offline/hermetic and never reads secrets, env, or config.
    let candidate =
        resolve_route_candidate(ApiProvider::XiaomiMimo, model_selector, None, None).ok()?;

    Some(FleetResolvedRoute {
        provider_id: candidate.provider_id.as_str().to_string(),
        provider_kind: candidate.provider_kind.as_str().to_string(),
        canonical_model: candidate
            .canonical_model
            .as_ref()
            .map(|model| model.as_str().to_string()),
        wire_model_id: candidate.wire_model_id.as_str().to_string(),
        protocol: route_protocol_label(candidate.protocol).to_string(),
        role,
        loadout: loadout_intent_label(&loadout),
        source: "resolver".to_string(),
    })
}

/// Plain-string label for a resolved wire protocol (no config type leaks).
fn route_protocol_label(protocol: mimofan_config::route::RequestProtocol) -> &'static str {
    use mimofan_config::route::RequestProtocol;
    match protocol {
        RequestProtocol::ChatCompletions => "chat_completions",
        RequestProtocol::Responses => "responses",
        RequestProtocol::AnthropicMessages => "anthropic_messages",
    }
}

/// Collapse an `inherit` (no-op) loadout to `None` for the receipt.
fn loadout_intent_label(loadout: &mimofan_config::FleetLoadout) -> Option<String> {
    if *loadout == mimofan_config::FleetLoadout::Inherit {
        None
    } else {
        Some(loadout.as_str().to_string())
    }
}

pub(crate) fn fleet_task_prompt(task_spec: &FleetTaskSpec) -> String {
    fleet_task_prompt_with_profile(task_spec, None)
}

pub(crate) fn fleet_task_prompt_with_profiles(
    task_spec: &FleetTaskSpec,
    agent_profiles: &[AgentProfile],
) -> Result<String> {
    let agent_profile = resolve_task_agent_profile(task_spec, agent_profiles)?;
    Ok(fleet_task_prompt_with_profile(task_spec, agent_profile))
}

fn fleet_task_prompt_with_profile(
    task_spec: &FleetTaskSpec,
    agent_profile: Option<&AgentProfile>,
) -> String {
    let mut prompt = String::new();
    prompt.push_str("Fleet task: ");
    prompt.push_str(&task_spec.name);

    if let Some(objective) = task_spec.objective.as_deref() {
        prompt.push_str("\n\nObjective:\n");
        prompt.push_str(objective);
    } else if let Some(description) = task_spec.description.as_deref() {
        prompt.push_str("\n\nObjective:\n");
        prompt.push_str(description);
    }

    prompt.push_str("\n\nInstructions:\n");
    prompt.push_str(&task_spec.instructions);

    if !task_spec.context.is_empty() {
        prompt.push_str("\n\nContext:\n");
        for item in &task_spec.context {
            prompt.push_str("- ");
            prompt.push_str(item);
            prompt.push('\n');
        }
    }

    if !task_spec.input_files.is_empty() {
        prompt.push_str("\nInput files:\n");
        for path in &task_spec.input_files {
            prompt.push_str("- ");
            prompt.push_str(&path.display().to_string());
            prompt.push('\n');
        }
    }

    if let Some(agent_profile) = agent_profile {
        prompt.push_str("\nFleet profile: ");
        prompt.push_str(&agent_profile.id);
        if let Some(display_name) = agent_profile.display_name.as_deref() {
            prompt.push_str(" (");
            prompt.push_str(display_name);
            prompt.push(')');
        }
        if let Some(description) = agent_profile.description.as_deref() {
            prompt.push_str("\nProfile description:\n");
            prompt.push_str(description);
        }
        if let Some(instructions) = agent_profile.profile.role.instructions.as_deref() {
            prompt.push_str("\nProfile instructions:\n");
            prompt.push_str(instructions);
        }
    }

    prompt
}

fn resolve_task_agent_profile<'a>(
    task_spec: &FleetTaskSpec,
    agent_profiles: &'a [AgentProfile],
) -> Result<Option<&'a AgentProfile>> {
    let Some(profile_id) = task_spec
        .worker
        .as_ref()
        .and_then(|worker| worker.agent_profile.as_deref())
        .map(str::trim)
        .filter(|id| !id.is_empty())
    else {
        return Ok(None);
    };
    let Some(profile) = agent_profiles
        .iter()
        .find(|profile| profile.id == profile_id)
    else {
        bail!(
            "fleet task {} references unknown agent profile {profile_id:?}",
            task_spec.id
        );
    };
    Ok(Some(profile))
}

fn effective_fleet_role(
    worker_profile: Option<&FleetTaskWorkerProfile>,
    agent_profile: Option<&AgentProfile>,
) -> Option<String> {
    worker_profile
        .and_then(|worker| worker.role.as_deref())
        .map(str::trim)
        .filter(|role| !role.is_empty())
        .map(str::to_string)
        .or_else(|| agent_profile.map(|profile| profile.profile.role.name.clone()))
}

fn effective_fleet_loadout(
    worker_profile: Option<&FleetTaskWorkerProfile>,
    agent_profile: Option<&AgentProfile>,
) -> mimofan_config::FleetLoadout {
    worker_profile
        .and_then(|worker| worker.model_class.as_deref().or(worker.loadout.as_deref()))
        .map(mimofan_config::FleetLoadout::from_name)
        .or_else(|| {
            agent_profile
                .map(|profile| profile.profile.loadout.clone())
                .filter(|loadout| *loadout != mimofan_config::FleetLoadout::Inherit)
        })
        .unwrap_or_default()
}

fn effective_fleet_model(
    run_model: &str,
    worker_profile: Option<&FleetTaskWorkerProfile>,
    agent_profile: Option<&AgentProfile>,
) -> String {
    worker_profile
        .and_then(|worker| worker.model.as_deref())
        .and_then(non_empty_trimmed)
        .or_else(|| {
            agent_profile
                .and_then(|profile| profile.profile.model.as_deref())
                .and_then(non_empty_trimmed)
        })
        .unwrap_or(run_model)
        .to_string()
}

/// Map a fleet role name to a `SubAgentType`. Unknown roles default to `General`.
fn fleet_role_to_agent_type(role: Option<&str>) -> SubAgentType {
    match role {
        Some("smoke-runner") => SubAgentType::Verifier,
        Some("scout") => SubAgentType::Explore,
        Some("read-only") => SubAgentType::Explore,
        Some("reviewer") => SubAgentType::Review,
        Some("builder") => SubAgentType::Implementer,
        Some("verifier") | Some("tester") => SubAgentType::Verifier,
        Some("planner") => SubAgentType::Plan,
        Some("explorer") => SubAgentType::Explore,
        Some("general") | None => SubAgentType::General,
        Some(other) => {
            // Try parsing as a SubAgentType directly
            SubAgentType::from_str(other).unwrap_or(SubAgentType::General)
        }
    }
}

/// Convert a fleet worker profile's tool list into an `AgentWorkerToolProfile`.
fn fleet_tool_profile(profile: Option<&FleetTaskWorkerProfile>) -> AgentWorkerToolProfile {
    match profile {
        Some(p) if !p.tools.is_empty() => AgentWorkerToolProfile::Explicit(p.tools.clone()),
        _ => AgentWorkerToolProfile::Inherited,
    }
}

fn fleet_worker_runtime_profile(
    agent_type: &SubAgentType,
    tool_profile: &AgentWorkerToolProfile,
    model: &str,
    spawn_depth: u32,
    max_spawn_depth: u32,
) -> WorkerRuntimeProfile {
    let mut profile = WorkerRuntimeProfile::for_role(agent_type.clone());
    profile.tools = match tool_profile {
        AgentWorkerToolProfile::Inherited => ToolScope::Inherit,
        AgentWorkerToolProfile::Explicit(tools) => ToolScope::Explicit(tools.clone()),
    };
    profile.model = if model == "auto" {
        ModelRoute::Auto
    } else {
        ModelRoute::Fixed(model.to_string())
    };
    profile.max_spawn_depth = max_spawn_depth.saturating_sub(spawn_depth);
    profile.background = true;
    profile
}

fn fleet_worker_runtime_profile_for_loadout(
    agent_type: &SubAgentType,
    tool_profile: &AgentWorkerToolProfile,
    model: &str,
    spawn_depth: u32,
    max_spawn_depth: u32,
    loadout: &mimofan_config::FleetLoadout,
) -> WorkerRuntimeProfile {
    let mut profile = fleet_worker_runtime_profile(
        agent_type,
        tool_profile,
        model,
        spawn_depth,
        max_spawn_depth,
    );
    profile.model = fleet_model_route_for_loadout(model, loadout);
    profile
}

fn non_empty_trimmed(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

fn fleet_model_route_for_loadout(
    model: &str,
    loadout: &mimofan_config::FleetLoadout,
) -> ModelRoute {
    let model = model.trim();
    if !model.is_empty() && !model.eq_ignore_ascii_case("auto") {
        return ModelRoute::Fixed(model.to_string());
    }
    match loadout {
        mimofan_config::FleetLoadout::Inherit => ModelRoute::Inherit,
        mimofan_config::FleetLoadout::Fast => ModelRoute::Faster,
        mimofan_config::FleetLoadout::Strong
        | mimofan_config::FleetLoadout::Balanced
        | mimofan_config::FleetLoadout::DeepReasoning
        | mimofan_config::FleetLoadout::Code
        | mimofan_config::FleetLoadout::Review
        | mimofan_config::FleetLoadout::ToolHeavy
        | mimofan_config::FleetLoadout::Custom(_) => ModelRoute::Auto,
    }
}

/// Create a fleet artifact ref from a worker output.
///
/// Uses the fleet artifact conventions: logs go under `.mimofan/fleet/`,
/// reports under `.mimofan/fleet/reports/`.
pub fn fleet_artifact_ref(
    _run_id: &str,
    _worker_id: &str,
    kind: mimofan_protocol::fleet::FleetArtifactKind,
    path: std::path::PathBuf,
) -> mimofan_protocol::fleet::FleetArtifactRef {
    mimofan_protocol::fleet::FleetArtifactRef {
        kind,
        path,
        checksum: None,
        mime_type: None,
        size_bytes: None,
    }
}

/// Map a sub-agent `AgentWorkerStatus` to a fleet `FleetWorkerEventPayload`.
///
/// This is the streaming bridge: as the sub-agent runs, each status transition
/// produces a corresponding fleet ledger event so the TUI surfaces stay in sync.
pub fn agent_status_to_fleet_event(
    status: AgentWorkerStatus,
    message: Option<&str>,
    tool_name: Option<&str>,
) -> FleetWorkerEventPayload {
    match status {
        AgentWorkerStatus::Queued => FleetWorkerEventPayload::Queued,
        AgentWorkerStatus::Starting => FleetWorkerEventPayload::Starting,
        AgentWorkerStatus::Running => FleetWorkerEventPayload::Running,
        AgentWorkerStatus::WaitingForUser => FleetWorkerEventPayload::ModelWait { model: None },
        AgentWorkerStatus::ModelWait => FleetWorkerEventPayload::ModelWait { model: None },
        AgentWorkerStatus::RunningTool => FleetWorkerEventPayload::RunningTool {
            tool: tool_name.unwrap_or("unknown").to_string(),
            call_id: None,
        },
        AgentWorkerStatus::Completed => FleetWorkerEventPayload::Completed {
            exit_code: Some(0),
            summary: message.map(|s| s.to_string()),
        },
        AgentWorkerStatus::Failed => FleetWorkerEventPayload::Failed {
            reason: message.unwrap_or("unknown error").to_string(),
            recoverable: false,
        },
        AgentWorkerStatus::Cancelled => FleetWorkerEventPayload::Cancelled { cancelled_by: None },
        AgentWorkerStatus::Interrupted => FleetWorkerEventPayload::Interrupted {
            signal: message.map(|s| s.to_string()),
        },
    }
}

/// Apply exec hardening to a worker spec from fleet config (#3027).
///
/// Filters tools against allowed/disallowed lists, caps max_steps to
/// config's max_turns, and returns the objective with system prompt
/// appended when configured.
pub fn apply_exec_hardening(
    mut spec: AgentWorkerSpec,
    exec: &mimofan_config::FleetExecConfig,
) -> AgentWorkerSpec {
    // Cap max_steps to config max_turns
    if exec.max_turns > 0 && exec.max_turns != u32::MAX {
        spec.max_steps = spec.max_steps.min(exec.max_turns);
    }
    spec.max_spawn_depth = exec
        .max_spawn_depth
        .min(mimofan_config::MAX_SPAWN_DEPTH_CEILING);
    spec.runtime_profile.max_spawn_depth = spec.max_spawn_depth.saturating_sub(spec.spawn_depth);

    // Apply tool filtering
    if !exec.allowed_tools.is_empty() || !exec.disallowed_tools.is_empty() {
        spec.tool_profile = filter_tool_profile(&spec.tool_profile, exec);
        spec.runtime_profile.tools = match &spec.tool_profile {
            AgentWorkerToolProfile::Inherited => ToolScope::Inherit,
            AgentWorkerToolProfile::Explicit(tools) => ToolScope::Explicit(tools.clone()),
        };
    }

    // Append system prompt
    if !exec.append_system_prompt.is_empty() {
        spec.objective = format!(
            "{}\n\n[Policy]\n{}",
            spec.objective, exec.append_system_prompt
        );
    }

    spec
}

/// Filter a tool profile against allowed/disallowed lists.
fn filter_tool_profile(
    profile: &AgentWorkerToolProfile,
    exec: &mimofan_config::FleetExecConfig,
) -> AgentWorkerToolProfile {
    match profile {
        AgentWorkerToolProfile::Explicit(tools) => {
            let filtered: Vec<String> = tools
                .iter()
                .filter(|t| {
                    // If allowed_tools is non-empty, only keep tools in the list
                    if !exec.allowed_tools.is_empty() && !exec.allowed_tools.contains(t) {
                        return false;
                    }
                    // Disallowed tools always win
                    !exec.disallowed_tools.contains(t)
                })
                .cloned()
                .collect();
            AgentWorkerToolProfile::Explicit(filtered)
        }
        AgentWorkerToolProfile::Inherited => {
            // Inherited profiles can't be filtered at spec time;
            // the sub-agent spawn path applies tool filtering.
            AgentWorkerToolProfile::Inherited
        }
    }
}

/// Determine whether a tool is safe for parallel execution (#2983).
///
/// Read-only tools that don't mutate state and have no side effects
/// are candidates for conservative parallel batching.
pub fn is_parallel_safe_read_only_tool(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "read_file"
            | "grep_files"
            | "file_search"
            | "list_dir"
            | "git_status"
            | "git_diff"
            | "git_log"
            | "git_show"
            | "git_blame"
            | "fetch_url"
            | "web_search"
            | "tool_search"
    )
}

#[cfg(test)]
mod tests {}
