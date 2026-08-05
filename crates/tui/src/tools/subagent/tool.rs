//! AgentTool — the single model-facing surface for sub-agent spawning.
//!
//! Extracted from `mod.rs` to keep the agent tool definition separate from the
//! manager, runtime, and registry internals.

use super::*;

/// Start a child agent task through a single simplified model-facing surface.
pub struct AgentTool {
    manager: SharedSubAgentManager,
    runtime: SubAgentRuntime,
}

impl AgentTool {
    #[must_use]
    pub fn new(manager: SharedSubAgentManager, runtime: SubAgentRuntime) -> Self {
        Self { manager, runtime }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AgentToolAction {
    Start,
    Status,
    Peek,
    Cancel,
}

fn parse_agent_tool_action(input: &Value) -> Result<AgentToolAction, ToolError> {
    let Some(action) = optional_input_str(input, &["action", "op"]) else {
        return Ok(AgentToolAction::Start);
    };
    match action.trim().to_ascii_lowercase().as_str() {
        "" | "start" | "spawn" | "run" => Ok(AgentToolAction::Start),
        "status" | "list" | "inspect" => Ok(AgentToolAction::Status),
        "peek" | "progress" => Ok(AgentToolAction::Peek),
        "cancel" | "stop" | "abort" => Ok(AgentToolAction::Cancel),
        other => Err(ToolError::invalid_input(format!(
            "Invalid agent action '{other}'. Use start, status, peek, or cancel."
        ))),
    }
}

fn parse_agent_ref(input: &Value) -> Option<String> {
    optional_input_str(input, &["agent_id", "id", "session_name", "name"])
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

#[async_trait]
impl ToolSpec for AgentTool {
    fn name(&self) -> &'static str {
        "agent"
    }

    fn description(&self) -> &'static str {
        concat!(
            "Start, inspect, peek at, or cancel focused child agent tasks through one surface. Use start only for independent work that benefits from a clean context. ",
            "For several independent targets, call agent separately for each target; mimofan runs or queues them under runtime capacity and provider rate-limit backpressure. ",
            "The child runs in the background and reports back automatically when finished; keep tiny reads/searches local. ",
            "Use action=status or action=peek with agent_id to inspect progress, and action=cancel with agent_id to stop a running child. Returns session projections with transcript_handle for UI/debug inspection."
        )
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["start", "status", "peek", "cancel"],
                    "description": "start (default) launches a child. status lists current children or inspects agent_id. peek is status for one child. cancel stops a running child by agent_id."
                },
                "agent_id": {
                    "type": "string",
                    "description": "Agent id or session name for action=status, action=peek, or action=cancel."
                },
                "include_archived": {
                    "type": "boolean",
                    "description": "For action=status without agent_id, include prior-session completed agents."
                },
                "name": {
                    "type": "string",
                    "description": "For action=start, optional stable session name. For status/peek/cancel, accepted as an alias for agent_id."
                },
                "prompt": {
                    "type": "string",
                    "description": "Focused task for the child agent. Prefer a compact Subagent Brief with QUESTION, SCOPE, ALREADY_KNOWN, EFFORT, STOP_CONDITION, and OUTPUT."
                },
                "type": {
                    "type": "string",
                    "description": SUBAGENT_TYPE_DESCRIPTION
                },
                "model_strength": {
                    "type": "string",
                    "enum": ["same", "faster"],
                    "description": "Optional child model strength. Use same when the child should be as capable as the current model. Use faster for type=explore, read-only lookup/search, status, or other low-risk tasks that can run on a smaller/faster same-family sibling; mimofan maps known families such as DeepSeek V4 Pro to Flash and GLM-5.2 to GLM-5-Turbo. type=explore defaults to faster unless you pass model_strength or model explicitly. No hidden auto-downgrade happens."
                },
                "model": {
                    "type": "string",
                    "description": "Optional exact provider model id for the child. Overrides model_strength. Prefer model_strength unless you know the provider-specific id."
                },
                "thinking": {
                    "type": "string",
                    "enum": ["inherit", "auto", "off", "low", "medium", "high", "max"],
                    "description": "Optional child thinking budget. inherit (default) follows the parent thinking mode. auto chooses from the child prompt. off is best for faster explore/lookups. high is for normal reasoning. max is for hard design/debug/release/security work. Explicit thinking overrides the default off used by model_strength=faster."
                },
                "cwd": {
                    "type": "string",
                    "description": "Optional pre-existing working directory for the child; must be inside the parent workspace. Prefer worktree=true for isolated parallel edit tasks."
                },
                "worktree": {
                    "type": "boolean",
                    "description": "When true, create a fresh git worktree and branch for this child before it starts. Use for parallel edit tasks that must not collide with the parent checkout."
                },
                "worktree_branch": {
                    "type": "string",
                    "description": "Optional branch name for worktree=true. Defaults to codex/agent-<name>-<id>."
                },
                "worktree_base": {
                    "type": "string",
                    "description": "Optional git ref to branch the worktree from. Defaults to HEAD in the parent checkout."
                },
                "worktree_path": {
                    "type": "string",
                    "description": "Optional worktree checkout path. Relative paths are created under the default sibling .mimofan-worktrees directory, not inside the parent checkout."
                },
                "fork_context": {
                    "type": "boolean",
                    "description": "false (default): fresh child context. true: include the current parent context prefix when the child needs it."
                },
                "max_depth": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 3,
                    "description": "Optional remaining nested-agent depth budget for this child. Defaults to the configured runtime budget."
                },
                "token_budget": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Optional aggregate token budget for this child and descendants. When unset, the child inherits the parent budget pool or the configured root default."
                }
            },
            "required": []
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![
            ToolCapability::ExecutesCode,
            ToolCapability::RequiresApproval,
        ]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Required
    }

    async fn execute(&self, input: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        let action = parse_agent_tool_action(&input)?;
        match action {
            AgentToolAction::Start => {}
            AgentToolAction::Status | AgentToolAction::Peek => {
                return inspect_agent_from_input(
                    &input,
                    self.manager.clone(),
                    context,
                    matches!(action, AgentToolAction::Peek),
                )
                .await;
            }
            AgentToolAction::Cancel => {
                return cancel_agent_from_input(&input, self.manager.clone(), context).await;
            }
        }
        let snapshot =
            spawn_subagent_from_input(input, self.manager.clone(), self.runtime.clone()).await?;
        let worker_record = {
            let manager = self.manager.read().await;
            manager.get_worker_record(&snapshot.agent_id)
        };
        let projection = subagent_session_projection(snapshot, false, context, worker_record).await;
        let mut tool_result = ToolResult::json(&projection)
            .map_err(|e| ToolError::execution_failed(e.to_string()))?;
        tool_result.metadata = Some(json!({
            "status": projection.status,
            "terminal": projection.terminal,
            "context_mode": projection.context_mode,
            "prefix_cache": projection.prefix_cache,
        }));
        Ok(tool_result)
    }
}

async fn inspect_agent_from_input(
    input: &Value,
    manager: SharedSubAgentManager,
    context: &ToolContext,
    peek: bool,
) -> Result<ToolResult, ToolError> {
    let include_archived =
        parse_optional_bool(input, &["include_archived", "includeArchived"]).unwrap_or(false);

    if let Some(agent_ref) = parse_agent_ref(input) {
        let (snapshot, worker_record) = {
            let manager = manager.read().await;
            let snapshot = manager
                .get_result_by_ref(&agent_ref)
                .map_err(|err| ToolError::invalid_input(err.to_string()))?;
            let worker_record = manager.get_worker_record(&snapshot.agent_id);
            (snapshot, worker_record)
        };
        let projection =
            subagent_session_projection(snapshot, include_archived, context, worker_record).await;
        let mut tool_result = ToolResult::json(&projection)
            .map_err(|err| ToolError::execution_failed(err.to_string()))?;
        tool_result.metadata = Some(json!({
            "action": if peek { "peek" } else { "status" },
            "status": projection.status,
            "terminal": projection.terminal,
            "agent_id": projection.agent_id,
        }));
        return Ok(tool_result);
    }

    let snapshots = {
        let manager = manager.read().await;
        manager
            .list_filtered(include_archived)
            .into_iter()
            .map(|snapshot| {
                let worker_record = manager.get_worker_record(&snapshot.agent_id);
                (snapshot, worker_record)
            })
            .collect::<Vec<_>>()
    };

    let mut projections = Vec::with_capacity(snapshots.len());
    for (snapshot, worker_record) in snapshots {
        projections.push(
            subagent_session_projection(snapshot, include_archived, context, worker_record).await,
        );
    }
    let payload = json!({
        "action": if peek { "peek" } else { "status" },
        "count": projections.len(),
        "agents": projections,
    });
    let mut tool_result =
        ToolResult::json(&payload).map_err(|err| ToolError::execution_failed(err.to_string()))?;
    tool_result.metadata = Some(json!({
        "action": if peek { "peek" } else { "status" },
        "count": payload["count"],
    }));
    Ok(tool_result)
}

async fn cancel_agent_from_input(
    input: &Value,
    manager: SharedSubAgentManager,
    context: &ToolContext,
) -> Result<ToolResult, ToolError> {
    let agent_ref = parse_agent_ref(input).ok_or_else(|| ToolError::missing_field("agent_id"))?;
    let (snapshot, worker_record) = {
        let mut manager = manager.write().await;
        let snapshot = manager
            .cancel_agent(&agent_ref)
            .map_err(|err| ToolError::invalid_input(err.to_string()))?;
        let worker_record = manager.get_worker_record(&snapshot.agent_id);
        (snapshot, worker_record)
    };
    let projection = subagent_session_projection(snapshot, false, context, worker_record).await;
    let mut tool_result = ToolResult::json(&projection)
        .map_err(|err| ToolError::execution_failed(err.to_string()))?;
    tool_result.metadata = Some(json!({
        "action": "cancel",
        "status": projection.status,
        "terminal": projection.terminal,
        "agent_id": projection.agent_id,
    }));
    Ok(tool_result)
}

async fn spawn_subagent_from_input(
    input: Value,
    manager: SharedSubAgentManager,
    runtime: SubAgentRuntime,
) -> Result<SubAgentResult, ToolError> {
    let spawn_request = parse_spawn_request(&input)?;

    if runtime.would_exceed_depth() {
        return Err(ToolError::execution_failed(format!(
            "Sub-agent depth limit reached (current depth {}, max {}). \
             Increase via [subagents] max_depth in config.toml.",
            runtime.spawn_depth, runtime.max_spawn_depth
        )));
    }

    if let Some(remaining) =
        crate::retry_status::rate_limit_remaining(runtime.client.api_provider().as_str())
    {
        let seconds = remaining.as_secs() + u64::from(remaining.subsec_nanos() > 0);
        return Err(ToolError::execution_failed(format!(
            "Provider is rate-limiting; sub-agent spawning is paused for {seconds}s. \
             Wait for the current backoff window before starting new agent work."
        )));
    }

    if spawn_request.worktree.is_some() {
        let manager_guard = manager.read().await;
        manager_guard
            .check_admission_capacity()
            .map_err(|err| ToolError::execution_failed(err.to_string()))?;
    }
    let child_workspace = prepare_child_workspace(&runtime.context.workspace, &spawn_request)?;

    let mut child_runtime = runtime.background_runtime();
    if let Some(max_depth) = spawn_request.max_depth {
        child_runtime.max_spawn_depth = child_runtime.spawn_depth.saturating_add(max_depth);
    }
    if let Some(workspace) = child_workspace {
        child_runtime.context.workspace = workspace;
    }
    let configured_model = match spawn_request.model.clone() {
        Some(model) => Some(normalize_requested_subagent_model(
            &model,
            "model",
            runtime.client.api_provider(),
        )?),
        None => configured_model_for_role_or_type(
            &runtime,
            spawn_request.assignment.role.as_deref(),
            &spawn_request.agent_type,
        )?,
    };
    let (effective_prompt, _resident_conflict) =
        if let Some(ref file_path) = spawn_request.resident_file {
            let abs_path = if std::path::Path::new(file_path).is_absolute() {
                std::path::PathBuf::from(file_path)
            } else {
                runtime.context.workspace.join(file_path)
            };
            let file_contents = std::fs::read_to_string(&abs_path)
                .unwrap_or_else(|e| format!("<!-- resident_file read error: {e} -->"));
            let prefixed = format!(
                "<!-- resident_file: {file_path} -->\n```\n{file_contents}\n```\n\n{}",
                spawn_request.prompt
            );
            let conflict = {
                let leases = RESIDENT_LEASES.get_or_init(|| std::sync::Mutex::new(HashMap::new()));
                let mut guard = leases.lock().unwrap_or_else(|p| p.into_inner());
                if let Some(owner) = guard.get(file_path) {
                    Some(format!(
                        "Warning: agent {owner} already holds a resident lease on {file_path}"
                    ))
                } else {
                    guard.insert(file_path.clone(), "pending".to_string());
                    None
                }
            };
            (prefixed, conflict)
        } else {
            (spawn_request.prompt, None)
        };

    let route = resolve_subagent_assignment_route(
        &runtime,
        configured_model,
        &effective_prompt,
        &spawn_request.agent_type,
        spawn_request.model_strength.model_route(),
        spawn_request.thinking,
    )
    .await;
    child_runtime.model = route.model.clone();
    child_runtime.reasoning_effort = route.reasoning_effort.clone();
    child_runtime.reasoning_effort_auto = false;
    let effective_model = route.model;
    let model_route = route.model_route;

    let mut manager_guard = manager.write().await;

    let result = manager_guard
        .spawn_background_with_assignment_options(
            Arc::clone(&manager),
            child_runtime,
            spawn_request.agent_type,
            effective_prompt,
            spawn_request.assignment,
            spawn_request.allowed_tools,
            SubAgentSpawnOptions {
                name: spawn_request.session_name.clone(),
                model: Some(effective_model),
                model_route: Some(model_route),
                nickname: None,
                fork_context: spawn_request.fork_context,
                token_budget: spawn_request.token_budget,
            },
            spawn_request.custom_agent_def,
        )
        .map_err(|e| ToolError::execution_failed(format!("Failed to spawn sub-agent: {e}")))?;

    if let Some(ref file_path) = spawn_request.resident_file
        && let Some(lock) = RESIDENT_LEASES.get()
        && let Ok(mut guard) = lock.lock()
        && let Some(owner) = guard.get_mut(file_path)
        && owner == "pending"
    {
        *owner = result.agent_id.clone();
    }

    Ok(result)
}
