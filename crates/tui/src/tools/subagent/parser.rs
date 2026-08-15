use super::*;

pub(crate) fn optional_input_str<'a>(input: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .filter_map(|key| input.get(*key).and_then(Value::as_str))
        .map(str::trim)
        .find(|value| !value.is_empty())
}

fn parse_text_or_items(
    input: &Value,
    text_keys: &[&str],
    items_key: &str,
    required_field: &str,
) -> Result<String, ToolError> {
    let text = optional_input_str(input, text_keys).map(str::to_string);
    let items = parse_items_text(input, items_key)?;
    match (text, items) {
        (Some(_), Some(_)) => Err(ToolError::invalid_input(format!(
            "Provide either {required_field} text or {items_key}, but not both"
        ))),
        (Some(text), None) => Ok(text),
        (None, Some(items)) => Ok(items),
        (None, None) => Err(ToolError::missing_field(required_field)),
    }
}

fn parse_items_text(input: &Value, key: &str) -> Result<Option<String>, ToolError> {
    let Some(items) = input.get(key) else {
        return Ok(None);
    };
    let array = items
        .as_array()
        .ok_or_else(|| ToolError::invalid_input(format!("'{key}' must be an array")))?;
    if array.is_empty() {
        return Err(ToolError::invalid_input(format!("'{key}' cannot be empty")));
    }

    let mut lines = Vec::new();
    for item in array {
        let object = item
            .as_object()
            .ok_or_else(|| ToolError::invalid_input("each item must be an object"))?;
        let item_type = object
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("text")
            .trim();
        let rendered = match item_type {
            "text" => object
                .get("text")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(str::to_string)
                .ok_or_else(|| ToolError::invalid_input("text item requires non-empty text"))?,
            "mention" => {
                let name = object
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .ok_or_else(|| ToolError::invalid_input("mention item requires name"))?;
                let path = object
                    .get("path")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .ok_or_else(|| ToolError::invalid_input("mention item requires path"))?;
                format!("[mention:${name}]({path})")
            }
            "skill" => {
                let name = object
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .ok_or_else(|| ToolError::invalid_input("skill item requires name"))?;
                let path = object
                    .get("path")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .ok_or_else(|| ToolError::invalid_input("skill item requires path"))?;
                format!("[skill:${name}]({path})")
            }
            "local_image" => {
                let path = object
                    .get("path")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .ok_or_else(|| ToolError::invalid_input("local_image item requires path"))?;
                format!("[local_image:{path}]")
            }
            "image" => {
                let url = object
                    .get("image_url")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .ok_or_else(|| ToolError::invalid_input("image item requires image_url"))?;
                format!("[image:{url}]")
            }
            _ => object
                .get("text")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| "[input]".to_string()),
        };
        lines.push(rendered);
    }

    Ok(Some(lines.join("\n")))
}

pub(crate) fn parse_spawn_request(input: &Value) -> Result<SpawnRequest, ToolError> {
    let prompt = parse_text_or_items(
        input,
        &["prompt", "message", "objective"],
        "items",
        "prompt",
    )?;
    let session_name = optional_input_str(input, &["name", "session_name"])
        .map(validate_session_name)
        .transpose()?;

    let type_input = optional_input_str(input, &["type", "agent_type", "agent_name"]);
    let role_input = optional_input_str(input, &["role", "agent_role"]);

    // Load custom agent registry once for lookup
    let custom_registry = custom_agents::CustomAgentRegistry::load();

    // Try to parse as built-in type, then check custom agents
    let mut custom_agent_def = None;
    let parsed_type = type_input
        .map(|kind| {
            // First try built-in types
            if let Some(t) = SubAgentType::from_str(kind) {
                return Ok(t);
            }
            // Then check custom agent definitions
            if let Some(def) = custom_registry.get(kind) {
                custom_agent_def = Some(def.clone());
                return Ok(SubAgentType::Custom);
            }
            Err(ToolError::invalid_input(format!(
                "Invalid sub-agent type '{kind}'. Use: {VALID_SUBAGENT_TYPES} or a custom agent name"
            )))
        })
        .transpose()?;

    let parsed_role_type = role_input
        .map(|role| {
            SubAgentType::from_str(role).ok_or_else(|| {
                ToolError::invalid_input(format!(
                    "Invalid role alias '{role}'. Use: {VALID_ROLE_ALIASES}"
                ))
            })
        })
        .transpose()?;

    if let (Some(type_kind), Some(role_kind)) = (&parsed_type, &parsed_role_type)
        && type_kind != role_kind
    {
        return Err(ToolError::invalid_input(
            "Conflicting type/agent_type and role/agent_role values".to_string(),
        ));
    }

    let agent_type = parsed_type
        .or(parsed_role_type)
        .unwrap_or(SubAgentType::General);

    if let Some(role) = role_input
        && normalize_role_alias(role).is_none()
    {
        return Err(ToolError::invalid_input(format!(
            "Invalid role alias '{role}'. Use: {VALID_ROLE_ALIASES}"
        )));
    }

    let role = role_input
        .and_then(normalize_role_alias)
        .or_else(|| type_input.and_then(normalize_role_alias))
        .map(str::to_string);

    // Use custom agent's tools if defined, otherwise use explicitly provided tools
    let allowed_tools = if let Some(ref def) = custom_agent_def {
        if def.tools.is_empty() {
            None // Empty = inherit all
        } else {
            Some(def.tools.clone())
        }
    } else {
        input
            .get("allowed_tools")
            .and_then(|v| v.as_array())
            .map(|items| {
                let mut tools = Vec::new();
                for item in items {
                    if let Some(tool) = item.as_str() {
                        let trimmed = tool.trim();
                        if !trimmed.is_empty() && !tools.iter().any(|existing| existing == trimmed)
                        {
                            tools.push(trimmed.to_string());
                        }
                    }
                }
                tools
            })
    };

    let cwd = parse_optional_cwd(input)?;
    let worktree = parse_optional_worktree_request(input)?;
    if cwd.is_some() && worktree.is_some() {
        return Err(ToolError::invalid_input(
            "Use either cwd or worktree isolation, not both".to_string(),
        ));
    }
    let model = parse_optional_subagent_model(input, "model")?;
    let model_strength = optional_input_str(input, &["model_strength", "modelStrength"])
        .map(SubAgentModelStrength::parse)
        .transpose()?
        .unwrap_or_else(|| {
            // Default model strength. `type: "explore"` defaults to Faster for
            // bounded read-only lookup/search/status work — the cheap, fast
            // same-family sibling is exactly the lossy-breadth job a child
            // should run. Every other role (and any call that supplies an
            // explicit `model`) stays conservative at Same. Explicit
            // model_strength above already wins via .parse(); explicit `model`
            // wins downstream in assignment_model_route regardless of strength.
            if agent_type == SubAgentType::Explore && model.is_none() {
                SubAgentModelStrength::Faster
            } else {
                SubAgentModelStrength::Same
            }
        });
    let thinking = optional_input_str(input, &["thinking", "reasoning_effort", "reasoningEffort"])
        .map(SubAgentThinking::parse)
        .transpose()?
        .unwrap_or(SubAgentThinking::Inherit);
    let resident_file = input
        .get("resident_file")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .filter(|s| !s.trim().is_empty());
    let fork_context =
        parse_optional_bool(input, &["fork_context", "forkContext", "inherit_context"])
            .unwrap_or(false);
    let fork_turns =
        parse_optional_positive_u64(input, &["fork_turns", "forkTurns", "window_turns"])?
            .map(|n| n as usize);
    let max_depth = input
        .get("max_depth")
        .or_else(|| input.get("maxDepth"))
        .or_else(|| input.get("max_spawn_depth"))
        .and_then(Value::as_u64)
        .map(|depth| {
            let ceiling = mimofan_config::MAX_SPAWN_DEPTH_CEILING;
            u32::try_from(depth)
                .map_err(|_| {
                    ToolError::invalid_input(format!("max_depth must be between 0 and {ceiling}"))
                })
                .and_then(|depth| {
                    if depth <= ceiling {
                        Ok(depth)
                    } else {
                        Err(ToolError::invalid_input(format!(
                            "max_depth must be between 0 and {ceiling}"
                        )))
                    }
                })
        })
        .transpose()?;
    let token_budget =
        parse_optional_positive_u64(input, &["token_budget", "tokenBudget", "max_tokens"])?;
    let trace_id = optional_input_str(input, &["trace_id", "traceId", "trace"])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    Ok(SpawnRequest {
        session_name,
        prompt: prompt.clone(),
        agent_type,
        assignment: SubAgentAssignment::new(prompt, role),
        allowed_tools,
        model: model.or_else(|| {
            custom_agent_def
                .as_ref()
                .filter(|d| d.model != "inherit")
                .map(|d| d.model.clone())
        }),
        model_strength,
        thinking,
        cwd,
        worktree,
        resident_file,
        fork_context,
        fork_turns,
        max_depth,
        token_budget,
        custom_agent_def,
        trace_id,
    })
}

fn validate_session_name(name: &str) -> Result<String, ToolError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(ToolError::invalid_input("name cannot be blank"));
    }
    if trimmed.chars().any(char::is_whitespace) {
        return Err(ToolError::invalid_input(
            "name must not contain whitespace; use letters, numbers, '-', '_', or '.'",
        ));
    }
    if !trimmed
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        return Err(ToolError::invalid_input(
            "name may only contain ASCII letters, numbers, '-', '_', or '.'",
        ));
    }
    Ok(trimmed.to_string())
}

pub(crate) fn parse_optional_bool(input: &Value, names: &[&str]) -> Option<bool> {
    names
        .iter()
        .find_map(|name| input.get(*name))
        .and_then(Value::as_bool)
}

fn parse_optional_positive_u64(input: &Value, names: &[&str]) -> Result<Option<u64>, ToolError> {
    for name in names {
        let Some(value) = input.get(*name) else {
            continue;
        };
        let Some(parsed) = value.as_u64() else {
            return Err(ToolError::invalid_input(format!(
                "{name} must be a positive integer token count"
            )));
        };
        if parsed == 0 {
            return Err(ToolError::invalid_input(format!(
                "{name} must be greater than zero; omit it to inherit or disable the budget"
            )));
        }
        return Ok(Some(parsed));
    }
    Ok(None)
}

pub(crate) fn normalize_requested_subagent_model(
    value: &str,
    field: &str,
    provider: crate::config::ApiProvider,
) -> Result<String, ToolError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(ToolError::invalid_input(format!("{field} cannot be blank")));
    }
    // #3018: Use provider-aware validation so non-DeepSeek providers can
    // accept their own model IDs instead of failing with "Expected a
    // DeepSeek model id".
    crate::config::requested_model_for_provider(provider, trimmed).ok_or_else(|| {
        let valid_names = crate::config::model_completion_names_for_provider(provider);
        let valid_hint = if valid_names.is_empty() {
            String::new()
        } else {
            format!(" (accepted: {})", valid_names.join(", "))
        };
        ToolError::invalid_input(format!(
            "Invalid {field} '{trimmed}' for provider {}{valid_hint}",
            provider_name_for_error(provider)
        ))
    })
}

fn provider_name_for_error(provider: crate::config::ApiProvider) -> &'static str {
    match provider {
        crate::config::ApiProvider::OpenAiCompatible => "OpenAI Compatible",
        _ => "this provider",
    }
}

pub(crate) fn configured_model_for_role_or_type(
    runtime: &SubAgentRuntime,
    role: Option<&str>,
    agent_type: &SubAgentType,
) -> Result<Option<String>, ToolError> {
    let mut keys = Vec::new();
    if let Some(role) = role.map(str::trim).filter(|role| !role.is_empty()) {
        keys.push(role.to_ascii_lowercase());
    }
    keys.push(agent_type.as_str().to_string());
    keys.push("default".to_string());

    for key in keys {
        if let Some(model) = runtime.role_models.get(&key) {
            return normalize_requested_subagent_model(
                model,
                &format!("subagents.{key}.model"),
                runtime.client.api_provider(),
            )
            .map(Some);
        }
    }
    Ok(None)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SubAgentResolvedRoute {
    pub(crate) model_route: ModelRoute,
    pub(crate) model: String,
    pub(crate) reasoning_effort: Option<String>,
    pub(crate) tuning: RequestTuning,
}

impl SubAgentResolvedRoute {
    fn new(
        model_route: ModelRoute,
        model: String,
        reasoning_effort: Option<String>,
    ) -> SubAgentResolvedRoute {
        let tuning = subagent_request_tuning(reasoning_effort.as_deref());
        SubAgentResolvedRoute {
            model_route,
            model,
            reasoning_effort,
            tuning,
        }
    }
}

pub(crate) async fn resolve_subagent_assignment_route(
    runtime: &SubAgentRuntime,
    configured_model: Option<String>,
    prompt: &str,
    agent_type: &SubAgentType,
    requested_model_route: ModelRoute,
    requested_thinking: SubAgentThinking,
) -> SubAgentResolvedRoute {
    let model_route = assignment_model_route(configured_model.as_deref(), requested_model_route);
    worker_profile_subagent_assignment_route(
        runtime,
        &model_route,
        requested_thinking,
        prompt,
        agent_type,
    )
}

fn assignment_model_route(
    configured_model: Option<&str>,
    requested_model_route: ModelRoute,
) -> ModelRoute {
    if let Some(model) = configured_model
        .map(str::trim)
        .filter(|model| !model.is_empty())
    {
        return ModelRoute::Fixed(model.to_string());
    }

    requested_model_route
}

fn subagent_request_tuning(reasoning_effort: Option<&str>) -> RequestTuning {
    RequestTuning {
        reasoning_effort: reasoning_effort.map(ReasoningEffort::from_setting),
        max_output_tokens: Some(SUBAGENT_RESPONSE_MAX_TOKENS),
    }
}

/// Candidate pair for explicit sub-agent strength routing, derived from the
/// active provider and the already provider-resolved parent model.
fn subagent_router_candidates(runtime: &SubAgentRuntime) -> crate::model_routing::RouterCandidates {
    crate::model_routing::provider_router_candidates(runtime.client.api_provider(), &runtime.model)
}

fn worker_profile_subagent_assignment_route(
    runtime: &SubAgentRuntime,
    model_route: &ModelRoute,
    requested_thinking: SubAgentThinking,
    prompt: &str,
    _agent_type: &SubAgentType,
) -> SubAgentResolvedRoute {
    let candidates = subagent_router_candidates(runtime);
    let mut requested_fast_lane = false;
    let model = match model_route {
        ModelRoute::Fixed(model) => model.clone(),
        ModelRoute::Faster | ModelRoute::Auto => {
            requested_fast_lane = true;
            candidates
                .cheap
                .clone()
                .unwrap_or_else(|| runtime.model.clone())
        }
        ModelRoute::Inherit => runtime.model.clone(),
    };

    let reasoning_effort = subagent_reasoning_effort_for_request(
        runtime,
        prompt,
        requested_fast_lane,
        requested_thinking,
    );

    SubAgentResolvedRoute::new(model_route.clone(), model, reasoning_effort)
}

fn subagent_reasoning_effort_for_request(
    runtime: &SubAgentRuntime,
    prompt: &str,
    requested_fast_lane: bool,
    requested_thinking: SubAgentThinking,
) -> Option<String> {
    match requested_thinking {
        SubAgentThinking::Effort(effort) => Some(effort.as_setting().to_string()),
        SubAgentThinking::Auto => Some(
            auto_subagent_reasoning_effort(prompt)
                .as_setting()
                .to_string(),
        ),
        SubAgentThinking::Inherit if requested_fast_lane => {
            // Faster/explore lane: cheaper reasoning by default. The OpenAI Codex
            // (GPT-5.5) adapter has no true "off" on the wire (it collapses off
            // to low), so we resolve Low honestly for that provider instead of
            // emitting an off that is silently rewritten. Explicit thinking
            // passed by the caller already won via the arms above.
            let provider = runtime.client.api_provider();
            let effort = if matches!(provider, crate::config::ApiProvider::OpenAiCompatible) {
                ReasoningEffort::Low
            } else {
                ReasoningEffort::Off
            };
            Some(effort.as_setting().to_string())
        }
        SubAgentThinking::Inherit => fallback_subagent_reasoning_effort(runtime, prompt),
    }
}

fn fallback_subagent_reasoning_effort(runtime: &SubAgentRuntime, prompt: &str) -> Option<String> {
    if runtime.reasoning_effort_auto {
        Some(
            auto_subagent_reasoning_effort(prompt)
                .as_setting()
                .to_string(),
        )
    } else {
        runtime.reasoning_effort.clone()
    }
}

fn auto_subagent_reasoning_effort(prompt: &str) -> ReasoningEffort {
    match crate::auto_reasoning::select(false, prompt) {
        ReasoningEffort::Low | ReasoningEffort::Medium => ReasoningEffort::High,
        other => other,
    }
}

fn parse_optional_subagent_model(input: &Value, key: &str) -> Result<Option<String>, ToolError> {
    match input.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                return Err(ToolError::invalid_input(format!("{key} cannot be blank")));
            }
            // #3018: Basic parsing only — provider-aware validation is deferred
            // to the spawn path where the runtime's ApiProvider is available.
            Ok(Some(trimmed.to_string()))
        }
        Some(_) => Err(ToolError::invalid_input(format!("{key} must be a string"))),
    }
}

/// Extract an optional `cwd: String` from spawn input and convert to a
/// `PathBuf`. Empty / absent → `None`. Workspace-boundary check happens
/// at spawn time (the parent's workspace is known there, not here).
fn parse_optional_cwd(input: &Value) -> Result<Option<PathBuf>, ToolError> {
    let raw = input.get("cwd").and_then(|v| v.as_str()).map(str::trim);
    match raw {
        None | Some("") => Ok(None),
        Some(s) => Ok(Some(PathBuf::from(s))),
    }
}

fn parse_optional_worktree_request(
    input: &Value,
) -> Result<Option<SubAgentWorktreeRequest>, ToolError> {
    let worktree_flag =
        parse_optional_bool_strict(input, &["worktree", "isolate_worktree", "isolateWorktree"])?;
    let isolation = optional_input_str(input, &["isolation"])
        .map(|value| value.trim().to_ascii_lowercase().replace(['_', '-'], ""));
    let isolation_wants_worktree = match isolation.as_deref() {
        None | Some("") | Some("none") | Some("shared") => false,
        Some("worktree") | Some("gitworktree") => true,
        Some(other) => {
            return Err(ToolError::invalid_input(format!(
                "isolation must be 'worktree' or 'none' (got '{other}')"
            )));
        }
    };

    let branch = optional_input_str(
        input,
        &[
            "worktree_branch",
            "worktreeBranch",
            "branch_name",
            "branchName",
            "branch",
        ],
    )
    .map(str::to_string);
    let path = optional_input_str(
        input,
        &[
            "worktree_path",
            "worktreePath",
            "worktree_dir",
            "worktreeDir",
        ],
    )
    .map(PathBuf::from);
    let base_ref = optional_input_str(
        input,
        &["worktree_base", "worktreeBase", "base_ref", "baseRef"],
    )
    .map(str::to_string);

    let has_worktree_details = branch.is_some() || path.is_some() || base_ref.is_some();
    if worktree_flag == Some(false) && (isolation_wants_worktree || has_worktree_details) {
        return Err(ToolError::invalid_input(
            "worktree=false conflicts with worktree isolation options".to_string(),
        ));
    }
    if worktree_flag.unwrap_or(false) || isolation_wants_worktree || has_worktree_details {
        Ok(Some(SubAgentWorktreeRequest {
            branch,
            path,
            base_ref,
        }))
    } else {
        Ok(None)
    }
}

fn parse_optional_bool_strict(input: &Value, names: &[&str]) -> Result<Option<bool>, ToolError> {
    for name in names {
        let Some(value) = input.get(*name) else {
            continue;
        };
        return value.as_bool().map(Some).ok_or_else(|| {
            ToolError::invalid_input(format!("{name} must be a boolean when provided"))
        });
    }
    Ok(None)
}

pub(crate) fn prepare_child_workspace(
    parent_workspace: &Path,
    request: &SpawnRequest,
) -> Result<Option<PathBuf>, ToolError> {
    if let Some(requested_cwd) = request.cwd.as_ref() {
        return validate_existing_child_cwd(parent_workspace, requested_cwd).map(Some);
    }
    if let Some(worktree) = request.worktree.as_ref() {
        return create_isolated_worktree(
            parent_workspace,
            worktree,
            request.session_name.as_deref(),
            &request.agent_type,
        )
        .map(Some);
    }
    Ok(None)
}

fn validate_existing_child_cwd(
    parent_workspace: &Path,
    requested_cwd: &Path,
) -> Result<PathBuf, ToolError> {
    let resolved = if requested_cwd.is_absolute() {
        requested_cwd.to_path_buf()
    } else {
        parent_workspace.join(requested_cwd)
    };
    let canonical = resolved.canonicalize().map_err(|e| {
        ToolError::invalid_input(format!(
            "Invalid cwd '{}': {e} (path may not exist yet — use worktree=true to let mimofan create an isolated checkout)",
            requested_cwd.display()
        ))
    })?;
    let workspace_canonical = parent_workspace
        .canonicalize()
        .unwrap_or_else(|_| parent_workspace.to_path_buf());
    if !canonical.starts_with(&workspace_canonical) {
        return Err(ToolError::invalid_input(format!(
            "cwd must be inside the parent workspace: {} is not under {}",
            canonical.display(),
            workspace_canonical.display()
        )));
    }
    Ok(canonical)
}

fn create_isolated_worktree(
    parent_workspace: &Path,
    request: &SubAgentWorktreeRequest,
    session_name: Option<&str>,
    agent_type: &SubAgentType,
) -> Result<PathBuf, ToolError> {
    let seed = session_name
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .or_else(|| Some(agent_type.as_str()));
    let spawn = crate::tools::worktree::service::WorktreeSpawnRequest {
        branch: request.branch.clone(),
        path: request.path.clone(),
        base_ref: request.base_ref.clone(),
        branch_seed: seed.map(str::to_string),
    };
    crate::tools::worktree::service::create_isolated_worktree(parent_workspace, &spawn)
}

/// Resolve a user-supplied role/agent_role value to a canonical role string.
///
/// This must accept the full set that [`SubAgentType::from_str`] accepts, plus
/// role-only aliases (`worker`, `default`, `awaiter`). Before #2649 it covered
/// only a subset, so `role: "reviewer"` (accepted by `from_str`) was rejected
/// here by the second validation pass with a misleading four-value hint.
fn normalize_role_alias(input: &str) -> Option<&'static str> {
    match input.to_ascii_lowercase().as_str() {
        "default" => Some("default"),
        "worker" | "general" | "general-purpose" | "general_purpose" => Some("worker"),
        "explorer" | "explore" | "exploration" => Some("explorer"),
        "awaiter" | "plan" | "planner" | "planning" => Some("awaiter"),
        "reviewer" | "review" | "code-review" | "code_review" => Some("reviewer"),
        "implementer" | "implement" | "implementation" | "builder" => Some("implementer"),
        "verifier" | "verify" | "verification" | "validator" | "tester" => Some("verifier"),
        "custom" => Some("custom"),
        _ => None,
    }
}

pub(crate) fn build_assignment_prompt(
    prompt: &str,
    assignment: &SubAgentAssignment,
    agent_type: &SubAgentType,
) -> String {
    let role = assignment.role.as_deref().unwrap_or("default");
    format!(
        "Assignment metadata:\n- objective: {}\n- role: {}\n- resolved_type: {}\n\nTask:\n{}",
        assignment.objective,
        role,
        agent_type.as_str(),
        prompt
    )
}

fn worker_status_from_subagent_status(status: &SubAgentStatus) -> AgentWorkerStatus {
    match status {
        SubAgentStatus::Running => AgentWorkerStatus::Running,
        SubAgentStatus::Completed => AgentWorkerStatus::Completed,
        SubAgentStatus::Failed(_) => AgentWorkerStatus::Failed,
        SubAgentStatus::Cancelled => AgentWorkerStatus::Cancelled,
        SubAgentStatus::BudgetExhausted => AgentWorkerStatus::Failed,
        SubAgentStatus::Interrupted(_) => AgentWorkerStatus::Interrupted,
    }
}

pub fn agent_worker_status_name(status: AgentWorkerStatus) -> &'static str {
    match status {
        AgentWorkerStatus::Queued => "queued",
        AgentWorkerStatus::Starting => "starting",
        AgentWorkerStatus::Running => "running",
        AgentWorkerStatus::WaitingForUser => "waiting_for_user",
        AgentWorkerStatus::ModelWait => "model_wait",
        AgentWorkerStatus::RunningTool => "running_tool",
        AgentWorkerStatus::Completed => "completed",
        AgentWorkerStatus::Failed => "failed",
        AgentWorkerStatus::Cancelled => "cancelled",
        AgentWorkerStatus::Interrupted => "interrupted",
    }
}

pub(crate) fn worker_status_from_subagent_result(result: &SubAgentResult) -> AgentWorkerStatus {
    if subagent_checkpoint_is_continuable(result) {
        AgentWorkerStatus::WaitingForUser
    } else {
        worker_status_from_subagent_status(&result.status)
    }
}

pub(crate) fn worker_progress_event_parts(
    message: &str,
) -> (AgentWorkerStatus, Option<u32>, Option<String>) {
    let step = parse_progress_step(message);
    let lower = message.to_ascii_lowercase();
    let status = if lower.contains("queued") {
        AgentWorkerStatus::Queued
    } else if lower.contains("waiting for user") || lower.contains("waiting for follow-up") {
        AgentWorkerStatus::WaitingForUser
    } else if lower.contains("requesting model response")
        || lower.contains(SUBAGENT_MODEL_WAIT_REASON)
    {
        AgentWorkerStatus::ModelWait
    } else if lower.contains("running tool") || lower.contains("executing") {
        AgentWorkerStatus::RunningTool
    } else if lower.contains("cancelled") {
        AgentWorkerStatus::Cancelled
    } else if lower.contains("interrupted") || lower.contains("timed out") {
        AgentWorkerStatus::Interrupted
    } else if lower.contains("complete") {
        AgentWorkerStatus::Completed
    } else if lower.contains("started") {
        AgentWorkerStatus::Starting
    } else {
        AgentWorkerStatus::Running
    };
    (status, step, parse_progress_tool_name(message))
}

fn parse_progress_step(message: &str) -> Option<u32> {
    let rest = message.strip_prefix("step ")?;
    let digits: String = rest.chars().take_while(|ch| ch.is_ascii_digit()).collect();
    (!digits.is_empty())
        .then(|| digits.parse::<u32>().ok())
        .flatten()
}

fn parse_progress_tool_name(message: &str) -> Option<String> {
    let marker = "tool '";
    let start = message.find(marker)? + marker.len();
    let rest = &message[start..];
    let end = rest.find('\'')?;
    let tool = rest[..end].trim();
    (!tool.is_empty()).then(|| tool.to_string())
}

pub(crate) fn subagent_progress_tool_display_name(name: &str) -> &str {
    match name {
        "exec_shell"
        | "exec_shell_wait"
        | "exec_shell_interact"
        | "task_shell_start"
        | "task_shell_wait" => "Bash",
        _ => name,
    }
}

pub(crate) fn emit_agent_progress(
    event_tx: Option<&mpsc::Sender<Event>>,
    agent_id: &str,
    status: String,
    parent_run_id: Option<String>,
    spawn_depth: u32,
) {
    if let Some(event_tx) = event_tx {
        let _ = event_tx.try_send(Event::AgentProgress {
            id: agent_id.to_string(),
            status,
            parent_run_id,
            spawn_depth,
        });
    }
}

// === Tool Registry Helpers ===

/// Per-sub-agent tool registry.
///
/// Two modes:
/// - **Full inheritance** (`allowed_tools = None`): the child sees the same
///   tool surface as the parent's Agent mode, except legacy sub-agent lifecycle
///   tools are removed. The single `agent` launcher remains visible only while
///   the configured depth budget allows another child. Approval-gated tools are
///   callable only when the parent runtime is auto-approved or, for explicit
///   write-capable roles (`implementer`, `custom`), when the tool's approval
///   requirement is `Suggest`.
/// - **Explicit narrow** (`allowed_tools = Some(list)`): legacy / Custom
///   path. The registry still builds the full surface, but only the listed
///   tool names are visible to the model and callable.
///
/// Pure per-role posture check (#3217), independent of any runtime: whether a
/// role may invoke a tool of the given approval level.
///
/// - Read (`Auto`) tools are always allowed.
/// - Write/edit/patch (`Suggest`) tools require a write-capable posture, so the
///   read-only roles (`explore`/`review`/`plan`/`verifier`) are denied.
/// - Shell (`Required`) tools require a `Full` shell posture, so only
///   `verifier`/`implementer`/`general` may shell out; `explore`/`review`
///   (read-only shell) and `plan` (no shell) are denied because read-only-shell
///   enforcement is not yet wired at the exec layer.
///
/// `custom` is governed by its explicit `allowed_tools` list, so the posture
/// check permits it here (the allowlist is the authority for that role).
pub(crate) fn role_posture_permits(
    agent_type: &SubAgentType,
    approval: ApprovalRequirement,
) -> bool {
    if matches!(agent_type, SubAgentType::Custom) {
        return true;
    }
    let profile = WorkerRuntimeProfile::for_role(agent_type.clone());
    match approval {
        ApprovalRequirement::Auto => true,
        ApprovalRequirement::Suggest => profile.permissions.write,
        ApprovalRequirement::Required => {
            matches!(profile.shell, crate::worker_profile::ShellPolicy::Full)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn spawn_request_carries_trace_id() {
        let input = json!({
            "prompt": "investigate the auth bug",
            "trace_id": "parent-turn-42",
        });
        let req = parse_spawn_request(&input).expect("parse must succeed");
        assert_eq!(
            req.trace_id.as_deref(),
            Some("parent-turn-42"),
            "trace_id must round-trip from input"
        );
    }

    #[test]
    fn spawn_request_trace_id_absent_by_default() {
        let input = json!({ "prompt": "do a thing" });
        let req = parse_spawn_request(&input).expect("parse must succeed");
        assert!(req.trace_id.is_none(), "trace_id defaults to None");
    }
}
