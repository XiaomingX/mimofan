//! Skills and MCP server route handlers.

use std::fs;
use std::path::{Path as FsPath, PathBuf};

use axum::Json;
use axum::extract::{Path, Query, State};

use crate::mcp::McpPool;

use super::RuntimeApiState;
use super::types::{
    ApiError, McpServerEntry, McpServersResponse, McpToolEntry, McpToolsQuery, McpToolsResponse,
    RuntimeInfoResponse, SetSkillEnabledRequest, SetSkillEnabledResponse, SkillEntry,
    SkillsResponse,
};

pub(crate) async fn runtime_info(
    State(state): State<RuntimeApiState>,
) -> Json<RuntimeInfoResponse> {
    let version = env!("CARGO_PKG_VERSION");
    Json(RuntimeInfoResponse {
        service: "mimofan-runtime-api",
        runtime_api_version: mimofan_protocol::runtime::RUNTIME_API_VERSION,
        mimofan_version: version,
        bind_host: state.bind_host.clone(),
        port: state.bind_port,
        auth_required: state.auth_required,
        transports: vec!["http", "sse"],
        capabilities: super::types::default_runtime_capabilities(),
        experimental: mimofan_protocol::runtime::RuntimeExperimentalCapabilities::default(),
        version,
    })
}

pub(crate) async fn list_skills(
    State(state): State<RuntimeApiState>,
) -> Result<Json<SkillsResponse>, ApiError> {
    let skills_dir = resolve_skills_dir(&state.config, &state.workspace);
    let mode = crate::skills::SkillDiscoveryMode::from_mimofan_only(
        state.config.skills_config().scan_mimofan_only(),
    );
    let (registry, directories) =
        discover_skills_for_runtime_api(&state.workspace, &skills_dir, mode);
    let skill_state = state.skill_state.lock().await;
    let skills = registry
        .list()
        .iter()
        .map(|skill| SkillEntry {
            name: skill.name.clone(),
            description: skill.description.clone(),
            path: skill.path.clone(),
            enabled: skill_state.is_enabled(&skill.name),
            is_bundled: skill_entry_is_bundled(skill, &skills_dir),
        })
        .collect();
    Ok(Json(SkillsResponse {
        directory: skills_dir,
        directories,
        warnings: registry.warnings().to_vec(),
        skills,
    }))
}

pub(crate) async fn set_skill_enabled(
    State(state): State<RuntimeApiState>,
    Path(name): Path<String>,
    Json(req): Json<SetSkillEnabledRequest>,
) -> Result<Json<SetSkillEnabledResponse>, ApiError> {
    let skills_dir = resolve_skills_dir(&state.config, &state.workspace);
    let mode = crate::skills::SkillDiscoveryMode::from_mimofan_only(
        state.config.skills_config().scan_mimofan_only(),
    );
    let (registry, directories) =
        discover_skills_for_runtime_api(&state.workspace, &skills_dir, mode);
    let exists = registry.list().iter().any(|skill| skill.name == name);
    if !exists {
        return Err(ApiError::not_found(format!(
            "skill '{name}' not found in searched directories: {}",
            format_skill_search_paths(&directories)
        )));
    }

    let mut store = state.skill_state.lock().await;
    store
        .set_enabled(&name, req.enabled)
        .map_err(|err| ApiError::internal(format!("persist skill state: {err}")))?;
    Ok(Json(SetSkillEnabledResponse {
        name,
        enabled: req.enabled,
    }))
}

pub(crate) async fn list_mcp_servers(
    State(state): State<RuntimeApiState>,
) -> Result<Json<McpServersResponse>, ApiError> {
    let config = crate::mcp::load_config_with_workspace(&state.mcp_config_path, &state.workspace)
        .map_err(|e| ApiError::internal(format!("Failed to load MCP config: {e}")))?;

    let mut servers = Vec::new();
    for (name, server_cfg) in config.servers {
        servers.push(McpServerEntry {
            name: name.clone(),
            enabled: server_cfg.is_enabled(),
            required: server_cfg.required,
            command: server_cfg.command.clone(),
            url: server_cfg.url.clone(),
            connected: false,
            enabled_tools: server_cfg.enabled_tools.clone(),
            disabled_tools: server_cfg.disabled_tools.clone(),
        });
    }
    servers.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(Json(McpServersResponse { servers }))
}

pub(crate) async fn list_mcp_tools(
    State(state): State<RuntimeApiState>,
    Query(query): Query<McpToolsQuery>,
) -> Result<Json<McpToolsResponse>, ApiError> {
    let mut pool_guard = state.mcp_pool.lock().await;
    if pool_guard.is_none() {
        let new_pool =
            McpPool::from_config_path_with_workspace(&state.mcp_config_path, &state.workspace)
                .map_err(|e| ApiError::internal(format!("Failed to load MCP config: {e}")))?;
        pool_guard.replace(new_pool);
    }
    // SAFETY: pool_guard is guaranteed to be Some after the initialization block above.
    let pool = pool_guard.as_mut().expect("pool initialized above");
    let _errors = pool.connect_all().await;

    let mut tools = Vec::new();
    for (prefixed_name, tool) in pool.all_tools() {
        let Ok((server, name)) = pool.parse_prefixed_name(&prefixed_name) else {
            continue;
        };

        if let Some(filter) = query.server.as_deref()
            && server != filter
        {
            continue;
        }

        tools.push(McpToolEntry {
            server: server.to_string(),
            name: name.to_string(),
            prefixed_name,
            description: tool.description.clone(),
            input_schema: tool.input_schema.clone(),
        });
    }

    tools.sort_by(|a, b| a.server.cmp(&b.server).then_with(|| a.name.cmp(&b.name)));

    Ok(Json(McpToolsResponse { tools }))
}

// ── Skill helper functions ──────────────────────────────────────────

pub(crate) fn resolve_skills_dir(config: &crate::config::Config, workspace: &FsPath) -> PathBuf {
    if config.skills_config().scan_mimofan_only() {
        if config.skills_dir.is_some() {
            return config.skills_dir();
        }
        if let Some(mimofan_skills_dir) = crate::skills::mimofan_workspace_skills_dir(workspace)
            && let Ok(canonical_skills) = fs::canonicalize(&mimofan_skills_dir)
        {
            return canonical_skills;
        }
        return config.skills_dir();
    }

    // Canonicalize the workspace once so the symlink-containment check below
    // compares like-for-like.
    let canonical_workspace = match fs::canonicalize(workspace) {
        Ok(path) => path,
        Err(_) => return config.skills_dir(),
    };
    for candidate in [
        canonical_workspace.join(".agents").join("skills"),
        canonical_workspace.join("skills"),
    ] {
        if let Ok(canon) = fs::canonicalize(&candidate)
            && canon.starts_with(&canonical_workspace)
            && canon.is_dir()
        {
            return canon;
        }
    }
    config.skills_dir()
}

fn skills_search_directories(
    workspace: &FsPath,
    skills_dir: &FsPath,
    mode: crate::skills::SkillDiscoveryMode,
) -> Vec<PathBuf> {
    crate::skills::skill_directories_for_workspace_and_dir(workspace, skills_dir, mode)
}

fn discover_skills_for_runtime_api(
    workspace: &FsPath,
    skills_dir: &FsPath,
    mode: crate::skills::SkillDiscoveryMode,
) -> (crate::skills::SkillRegistry, Vec<PathBuf>) {
    let directories = skills_search_directories(workspace, skills_dir, mode);
    let registry = crate::skills::discover_from_directories(directories.clone());
    (registry, directories)
}

fn skill_entry_is_bundled(skill: &crate::skills::Skill, skills_dir: &FsPath) -> bool {
    if !crate::skills::is_bundled_skill_name(&skill.name) {
        return false;
    }

    let expected_path = skills_dir.join(&skill.name).join("SKILL.md");
    paths_refer_to_same_file(&skill.path, &expected_path)
}

fn paths_refer_to_same_file(left: &FsPath, right: &FsPath) -> bool {
    match (fs::canonicalize(left), fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

fn format_skill_search_paths(directories: &[PathBuf]) -> String {
    if directories.is_empty() {
        return "<none>".to_string();
    }
    directories
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}
