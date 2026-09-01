//! Exec / one-shot agent functions extracted from `lib.rs`.
//!
//! Contains `run_exec_agent`, `run_one_shot`, `run_one_shot_json`,
//! sandbox commands, patch application, and supporting helpers.

use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use tempfile::NamedTempFile;
use wait_timeout::ChildExt;

use crate::cli::{ApplyArgs, ExecOutputFormat, SandboxArgs, SandboxCommand};
use crate::config::{Config, MAX_SUBAGENTS};
use crate::dependencies::ExternalTool;
use crate::llm_client::LlmClient;
use crate::models::{ContentBlock, Message, MessageRequest, SystemPrompt};
use crate::session_manager::{SessionManager, truncate_id};

// ---------------------------------------------------------------------------
// Patch application
// ---------------------------------------------------------------------------

pub(crate) fn run_apply(args: ApplyArgs) -> Result<()> {
    let patch = if let Some(path) = args.patch_file {
        std::fs::read_to_string(&path)
            .map_err(|e| anyhow::anyhow!("Failed to read patch {}: {}", path.display(), e))?
    } else {
        read_patch_from_stdin()?
    };
    if patch.trim().is_empty() {
        bail!("Patch is empty.");
    }

    let mut tmp = NamedTempFile::new()?;
    tmp.write_all(patch.as_bytes())?;
    let tmp_path = tmp.path().to_path_buf();

    let output = crate::dependencies::Git::command()
        .ok_or_else(|| anyhow::anyhow!("git not found on PATH"))?
        .arg("apply")
        .arg("--whitespace=nowarn")
        .arg(&tmp_path)
        .output()
        .map_err(|e| anyhow::anyhow!("Failed to run git apply: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git apply failed: {}", stderr.trim());
    }
    println!("Applied patch successfully.");
    Ok(())
}

fn read_patch_from_stdin() -> Result<String> {
    let mut stdin = io::stdin();
    if stdin.is_terminal() {
        bail!("No patch file provided and stdin is empty.");
    }
    let mut buffer = String::new();
    stdin.read_to_string(&mut buffer)?;
    Ok(buffer)
}

// ---------------------------------------------------------------------------
// Sandbox command execution
// ---------------------------------------------------------------------------

pub(crate) fn run_sandbox_command(args: SandboxArgs) -> Result<()> {
    use crate::sandbox::{CommandSpec, OsSandbox};

    let SandboxCommand::Run {
        policy,
        network,
        writable_root,
        exclude_tmpdir,
        exclude_slash_tmp,
        cwd,
        timeout_ms,
        command,
    } = args.command;

    let policy = parse_sandbox_policy(
        &policy,
        network,
        writable_root,
        exclude_tmpdir,
        exclude_slash_tmp,
    )?;
    let cwd = cwd.unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let timeout = Duration::from_millis(timeout_ms.clamp(1000, 600_000));

    let (program, args) = command
        .split_first()
        .ok_or_else(|| anyhow::anyhow!("Command is required"))?;
    let spec =
        CommandSpec::program(program, args.to_vec(), cwd.clone(), timeout).with_policy(policy);
    let manager = OsSandbox::new();
    let exec_env = manager.prepare(&spec);

    let mut cmd = Command::new(exec_env.program());
    cmd.args(exec_env.args())
        .current_dir(&exec_env.cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    crate::child_env::apply_to_command(&mut cmd, crate::child_env::string_map_env(&exec_env.env));

    let mut child = cmd
        .spawn()
        .map_err(|e| anyhow::anyhow!("Failed to run command: {e}"))?;
    let stdout_handle = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("stdout unavailable"))?;
    let stderr_handle = child
        .stderr
        .take()
        .ok_or_else(|| anyhow::anyhow!("stderr unavailable"))?;

    let timeout = exec_env.timeout;
    let stdout_thread = std::thread::spawn(move || {
        let mut reader = stdout_handle;
        let mut buf = Vec::new();
        let _ = reader.read_to_end(&mut buf);
        buf
    });
    let stderr_thread = std::thread::spawn(move || {
        let mut reader = stderr_handle;
        let mut buf = Vec::new();
        let _ = reader.read_to_end(&mut buf);
        buf
    });

    if let Some(status) = child.wait_timeout(timeout)? {
        let stdout = stdout_thread.join().unwrap_or_default();
        let stderr = stderr_thread.join().unwrap_or_default();
        let stderr_str = String::from_utf8_lossy(&stderr);
        let exit_code = status.code().unwrap_or(-1);
        let sandbox_type = exec_env.sandbox_type;
        let sandbox_denied = OsSandbox::was_denied(sandbox_type, exit_code, &stderr_str);

        if !stdout.is_empty() {
            print!("{}", String::from_utf8_lossy(&stdout));
        }
        if !stderr.is_empty() {
            eprint!("{stderr_str}");
        }
        if sandbox_denied {
            eprintln!("{}", OsSandbox::denial_message(sandbox_type, &stderr_str));
        }

        if !status.success() {
            bail!("Command failed with exit code {exit_code}");
        }
    } else {
        let _ = child.kill();
        let _ = child.wait();
        bail!("Command timed out after {}ms", timeout.as_millis());
    }
    Ok(())
}

fn parse_sandbox_policy(
    policy: &str,
    network: bool,
    writable_root: Vec<PathBuf>,
    exclude_tmpdir: bool,
    exclude_slash_tmp: bool,
) -> Result<crate::sandbox::SandboxPolicy> {
    use crate::sandbox::SandboxPolicy;

    match policy {
        "danger-full-access" => Ok(SandboxPolicy::DangerFullAccess),
        "read-only" => Ok(SandboxPolicy::ReadOnly),
        "external-sandbox" => Ok(SandboxPolicy::ExternalSandbox {
            network_access: network,
        }),
        "workspace-write" => Ok(SandboxPolicy::WorkspaceWrite {
            writable_roots: writable_root,
            network_access: network,
            exclude_tmpdir,
            exclude_slash_tmp,
        }),
        other => bail!("Unknown sandbox policy: {other}"),
    }
}

// ---------------------------------------------------------------------------
// CLI auto-route resolution
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub(crate) struct CliAutoRoute {
    pub(crate) provider: crate::config::ApiProvider,
    pub(crate) model: String,
    pub(crate) reasoning_effort: Option<crate::tui::app::ReasoningEffort>,
    pub(crate) auto_model: bool,
}

pub(crate) fn cli_reasoning_effort_value(
    config: &Config,
    effort: crate::tui::app::ReasoningEffort,
) -> Option<String> {
    effort
        .api_value_for_provider(config.api_provider())
        .map(str::to_string)
}

pub(crate) fn config_for_cli_route(config: &Config, route: &CliAutoRoute) -> Config {
    let mut execution_config = config.clone();
    execution_config.provider = Some(route.provider.as_str().to_string());
    execution_config
        .provider_config_for_mut(route.provider)
        .model = Some(route.model.clone());
    if matches!(route.provider, crate::config::ApiProvider::OpenAiCompatible) {
        execution_config.default_text_model = Some(route.model.clone());
    }
    execution_config
}

pub(crate) async fn resolve_cli_auto_route(
    config: &Config,
    model: &str,
    prompt: &str,
) -> Result<CliAutoRoute> {
    if model.trim().eq_ignore_ascii_case("auto") {
        let selection = crate::model_routing::resolve_auto_route_with_inventory(
            config, prompt, "", "auto", "auto",
        )
        .await?;
        Ok(CliAutoRoute {
            provider: selection.provider,
            model: selection.model,
            reasoning_effort: selection.reasoning_effort,
            auto_model: true,
        })
    } else {
        if let Some(selection) =
            crate::model_routing::resolve_explicit_route_with_inventory(config, model)
        {
            return Ok(CliAutoRoute {
                provider: selection.provider,
                model: selection.model,
                reasoning_effort: selection.reasoning_effort,
                auto_model: false,
            });
        }

        let candidate_providers =
            crate::model_routing::explicit_route_candidate_providers(config, model);
        if !candidate_providers.is_empty() && !candidate_providers.contains(&config.api_provider())
        {
            let providers = candidate_providers
                .iter()
                .map(|provider| provider.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            bail!(
                "model `{model}` is available from configured provider route(s): {providers}. \
                 Pass `--provider <provider>` with `--model {model}` to choose one explicitly."
            );
        }

        // When --model is not `auto`, fall back to the reasoning_effort
        // declared in the user's config.toml. The previous hard-coded `None`
        // silently dropped the user's setting on every non-auto-route exec
        // call, which (for example) prevented custom-endpoint users from
        // disabling thinking via `reasoning_effort = "off"` and caused
        // 30+ second SSE idle timeouts on trivial prompts.
        Ok(CliAutoRoute {
            provider: config.api_provider(),
            model: model.to_string(),
            reasoning_effort: config
                .reasoning_effort()
                .map(crate::tui::app::ReasoningEffort::from_setting),
            auto_model: false,
        })
    }
}

// ---------------------------------------------------------------------------
// One-shot execution
// ---------------------------------------------------------------------------

pub(crate) async fn run_one_shot(config: &Config, model: &str, prompt: &str) -> Result<()> {
    use crate::client::ApiClient;

    let route = resolve_cli_auto_route(config, model, prompt).await?;
    let execution_config = config_for_cli_route(config, &route);
    let client = ApiClient::new_detached(&execution_config)?;
    let reasoning_effort = route
        .reasoning_effort
        .and_then(|effort| cli_reasoning_effort_value(&execution_config, effort));

    let request = MessageRequest {
        model: route.model,
        messages: vec![Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: prompt.to_string(),
                cache_control: None,
            }],
        }],
        max_tokens: 4096,
        system: None,
        tools: None,
        tool_choice: None,
        metadata: None,
        thinking: None,
        reasoning_effort,
        stream: Some(false),
        temperature: None,
        top_p: None,
        response_format: None,
    };

    let response = client.create_message(request).await?;

    for block in response.content {
        if let ContentBlock::Text { text, .. } = block {
            println!("{text}");
        }
    }

    Ok(())
}

pub(crate) async fn run_one_shot_json(config: &Config, model: &str, prompt: &str) -> Result<()> {
    use crate::client::ApiClient;

    let route = resolve_cli_auto_route(config, model, prompt).await?;
    let execution_config = config_for_cli_route(config, &route);
    let client = ApiClient::new_detached(&execution_config)?;
    let model = route.model.clone();
    let reasoning_effort = route
        .reasoning_effort
        .and_then(|effort| cli_reasoning_effort_value(&execution_config, effort));
    let request = MessageRequest {
        model: model.clone(),
        messages: vec![Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: prompt.to_string(),
                cache_control: None,
            }],
        }],
        max_tokens: 4096,
        system: Some(SystemPrompt::Text(
            include_str!("../prompts/coding_assistant.md")
                .trim()
                .to_string(),
        )),
        tools: None,
        tool_choice: None,
        metadata: None,
        thinking: None,
        reasoning_effort,
        stream: Some(false),
        temperature: Some(0.2),
        top_p: Some(0.9),
        response_format: None,
    };

    let response = client.create_message(request).await?;
    let mut output = String::new();
    for block in response.content {
        if let ContentBlock::Text { text, .. } = block {
            output.push_str(&text);
        }
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "mode": "one-shot",
            "model": model,
            "success": true,
            "output": output
        }))?
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Exec stream metadata / events
// ---------------------------------------------------------------------------

#[derive(serde::Serialize)]
pub(crate) struct ExecStreamMeta {
    model: String,
    input_tokens: u32,
    output_tokens: u32,
    session_id: String,
    resume_command: String,
    workspace: String,
    message_count: usize,
    status: Option<String>,
}

#[derive(serde::Serialize)]
#[serde(tag = "type")]
pub(crate) enum ExecStreamEvent {
    #[serde(rename = "content")]
    Content { content: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        name: String,
        id: String,
        input: serde_json::Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        id: String,
        output: String,
        status: String,
    },
    #[serde(rename = "session_capture")]
    SessionCapture { content: String },
    #[serde(rename = "metadata")]
    Metadata { meta: ExecStreamMeta },
    #[serde(rename = "done")]
    Done,
    #[serde(rename = "error")]
    Error { error: String },
}

fn emit_exec_stream_event(event: &ExecStreamEvent) -> Result<()> {
    println!("{}", serde_json::to_string(event)?);
    Ok(())
}

fn exec_saved_session_line(session_id: &str) -> String {
    format!("session: {}", truncate_id(session_id))
}

fn exec_resumed_session_line(session_id: &str) -> String {
    format!("resumed session: {}", truncate_id(session_id))
}

fn exec_stream_session_ref(session_id: &str) -> String {
    crate::utils::redacted_identifier_for_log(session_id)
}

fn exec_stream_resume_hint(session_id: &str) -> String {
    if session_id.trim().is_empty() {
        String::new()
    } else {
        "mimofan exec --resume <redacted-session-id>".to_string()
    }
}

// ---------------------------------------------------------------------------
// Session persistence
// ---------------------------------------------------------------------------

fn persist_exec_session(
    messages: &[Message],
    model: &str,
    workspace: &Path,
    system_prompt: &Option<SystemPrompt>,
    session_id: Option<&str>,
    total_tokens: u64,
) -> Result<String> {
    let manager =
        SessionManager::default_location().context("could not open session manager for save")?;
    let saved = if let Some(id) = session_id.filter(|id| !id.trim().is_empty()) {
        match manager.load_session(id) {
            Ok(existing) => crate::session_manager::update_session(
                existing,
                messages,
                total_tokens,
                system_prompt.as_ref(),
            ),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                crate::session_manager::create_saved_session_with_id_and_mode(
                    id.to_string(),
                    messages,
                    model,
                    workspace,
                    total_tokens,
                    system_prompt.as_ref(),
                    Some("exec"),
                )
            }
            Err(err) => return Err(err).context("could not load existing exec session"),
        }
    } else {
        crate::session_manager::create_saved_session_with_mode(
            messages,
            model,
            workspace,
            total_tokens,
            system_prompt.as_ref(),
            Some("exec"),
        )
    };
    let id = saved.metadata.id.clone();
    manager
        .save_session(&saved)
        .context("could not save exec session")?;
    Ok(id)
}

// ---------------------------------------------------------------------------
// Full exec agent run
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_exec_agent(
    config: &Config,
    model: &str,
    prompt: &str,
    workspace: PathBuf,
    max_subagents: usize,
    auto_approve: bool,
    trust_mode: bool,
    json_output: bool,
    resume_session_id: Option<String>,
    output_format: ExecOutputFormat,
    max_turns: u32,
    allowed_tools: Option<Vec<String>>,
    disallowed_tools: Option<Vec<String>>,
    append_system_prompt: Option<String>,
    json_schema: Option<String>,
    unattended: bool,
) -> Result<()> {
    use crate::compaction::CompactionConfig;
    use crate::core::engine::{EngineConfig, spawn_engine};
    use crate::core::events::Event;
    use crate::core::ops::Op;
    use crate::models::{
        auto_compact_default_for_model, compaction_threshold_for_model_at_percent,
    };
    use crate::tools::plan::new_shared_plan_state;
    use crate::tools::todo::new_shared_todo_list;
    use crate::tui::app::AppMode;

    let route = resolve_cli_auto_route(config, model, prompt).await?;
    let execution_config = config_for_cli_route(config, &route);
    let auto_model = route.auto_model;
    let effective_provider = route.provider;
    let effective_model = route.model;
    let max_subagents = if max_subagents == config.max_subagents_for_provider(config.api_provider())
    {
        execution_config
            .max_subagents_for_provider(effective_provider)
            .clamp(1, MAX_SUBAGENTS)
    } else {
        max_subagents
    };
    let effective_reasoning_effort = route
        .reasoning_effort
        .and_then(|effort| cli_reasoning_effort_value(&execution_config, effort));

    let settings = crate::settings::Settings::load().unwrap_or_default();
    let auto_compact_enabled = if crate::settings::Settings::auto_compact_explicitly_configured() {
        settings.auto_compact
    } else {
        auto_compact_default_for_model(&effective_model)
    };
    let compaction = CompactionConfig {
        enabled: auto_compact_enabled,
        model: effective_model.clone(),
        token_threshold: compaction_threshold_for_model_at_percent(
            &effective_model,
            settings.compact_threshold,
        ),
        custom_instructions: crate::project_context::load_project_context(&workspace)
            .compact_instructions(),
        ..Default::default()
    };

    let network_policy = execution_config.network.clone().map(|toml_cfg| {
        crate::network_policy::NetworkPolicyDecider::with_default_audit(toml_cfg.into_runtime())
    });

    let lsp_config = execution_config
        .lsp
        .clone()
        .map(crate::config::LspConfigToml::into_runtime);

    // `--json-schema` (#824): build the synthetic terminator tool and a shared
    // submission slot. The tool is registered into the engine only when the
    // flag is supplied; on a schema-valid submission it records the payload in
    // `json_schema_submission` and the exec loop below terminates the run.
    let (json_schema_terminator, json_schema_submission) = match json_schema {
        Some(ref raw) => {
            let schema = crate::tools::json_schema_terminator::parse_json_schema_arg(raw)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            let submission: crate::tools::json_schema_terminator::SubmissionSlot =
                std::sync::Arc::new(std::sync::Mutex::new(None));
            let tools: Vec<std::sync::Arc<dyn crate::tools::spec::ToolSpec>> =
                vec![std::sync::Arc::new(
                    crate::tools::json_schema_terminator::JsonSchemaTerminator::new(
                        schema,
                        submission.clone(),
                    ),
                )];
            (tools, submission)
        }
        None => (Vec::new(), std::sync::Arc::new(std::sync::Mutex::new(None))),
    };

    // #863 — validate unattended/headless coherence *before* starting the
    // engine. Fail fast here (rather than mid-run) when the configuration
    // cannot guarantee a safe, terminating headless run. The gate inspects the
    // task budget (env/config), the max-turn cap, and the failure log path.
    if unattended {
        use crate::core::engine::headless_gate::{HeadlessGate, HeadlessGateConfig};
        let task_budget_tokens = std::env::var("MIMOFAN_UNATTENDED_TASK_BUDGET")
            .ok()
            .and_then(|v| v.trim().parse::<usize>().ok());
        let failure_log_path = std::env::var("MIMOFAN_UNATTENDED_FAILURE_LOG")
            .ok()
            .map(PathBuf::from);
        let mut gate = HeadlessGate::new(HeadlessGateConfig {
            unattended: true,
            task_budget_tokens,
            max_steps: max_turns,
            failure_log_path,
        });
        gate.validate(&workspace)
            .map_err(|e| anyhow::anyhow!("headless gate rejected unattended run: {e}"))?;
    }

    let engine_config = EngineConfig {
        model: effective_model.clone(),
        active_route_limits: None,
        workspace: workspace.clone(),
        allow_shell: auto_approve || execution_config.allow_shell(),
        trust_mode,
        notes_path: execution_config.notes_path(),
        mcp_config_path: execution_config.mcp_config_path(),
        skills_dir: execution_config.skills_dir(),
        skills_scan_mimofan_only: execution_config.skills_config().scan_mimofan_only(),
        instructions: {
            let mut instrs: Vec<crate::prompts::InstructionSource> = execution_config
                .instructions_paths()
                .into_iter()
                .map(Into::into)
                .collect();
            if let Some(ref extra) = append_system_prompt {
                instrs.push(crate::prompts::InstructionSource::Inline {
                    name: "cli:append-system-prompt".into(),
                    content: extra.clone(),
                });
            }
            instrs
        },
        project_context_pack_enabled: execution_config.project_context_pack_enabled(),
        git_status_in_prompt: execution_config.git_status_in_prompt(),
        translation_enabled: false,
        show_thinking: settings.show_thinking,
        max_steps: max_turns,
        max_subagents,
        max_admitted_subagents: execution_config
            .max_admitted_subagents_for_provider(effective_provider)
            .max(max_subagents),
        launch_concurrency: execution_config.launch_concurrency_for_provider(effective_provider),
        subagents_enabled: execution_config.subagents_enabled_for_provider(effective_provider),
        features: execution_config.features(),
        auto_review_policy: execution_config.auto_review_policy(),
        compaction,
        todos: new_shared_todo_list(),
        plan_state: new_shared_plan_state(),
        goal_queue: crate::tools::goal::new_shared_goal_queue(),
        max_spawn_depth: execution_config.subagent_max_spawn_depth_for_provider(effective_provider),
        subagent_token_budget: execution_config
            .subagent_token_budget_for_provider(effective_provider),
        network_policy,
        snapshots_enabled: execution_config.snapshots_config().enabled,
        snapshots_max_workspace_bytes: execution_config
            .snapshots_config()
            .max_workspace_gb
            .saturating_mul(1024 * 1024 * 1024),
        lsp_config,
        runtime_services: crate::tools::spec::RuntimeToolServices::default(),
        subagent_model_overrides: execution_config.subagent_model_overrides(),
        subagent_api_timeout: std::time::Duration::from_secs(
            execution_config.subagent_api_timeout_secs_for_provider(effective_provider),
        ),
        stream_chunk_timeout: std::time::Duration::from_secs(
            execution_config.stream_chunk_timeout_secs(),
        ),
        subagent_heartbeat_timeout: std::time::Duration::from_secs(
            execution_config.subagent_heartbeat_timeout_secs_for_provider(effective_provider),
        ),
        prefer_bwrap: execution_config.prefer_bwrap.unwrap_or(false),
        memory_enabled: execution_config.memory_enabled(),
        memory_dir: execution_config.memory_dir(),
        speech_output_dir: execution_config.speech_output_dir(),
        vision_config: execution_config.vision_model_config(),
        strict_tool_mode: execution_config.strict_tool_mode.unwrap_or(false),
        goal_objective: None,
        goal_token_budget: None,
        goal_status: crate::tools::goal::GoalStatus::Active,
        allowed_tools: allowed_tools.clone(),
        disallowed_tools: disallowed_tools.clone(),
        hook_executor: None,
        locale_tag: crate::localization::resolve_locale(&settings.locale)
            .tag()
            .to_string(),
        workshop: config.workshop.clone(),
        search_provider: execution_config.search_provider(),
        search_api_key: execution_config
            .search
            .as_ref()
            .and_then(|s| s.api_key.clone()),
        search_base_url: execution_config
            .search
            .as_ref()
            .and_then(|s| s.base_url.clone()),
        tools_always_load: execution_config.tools_always_load(),
        tools: execution_config.tools.clone(),
        verbosity: execution_config.verbosity.clone(),
        workspace_follow_symlinks: settings.workspace_follow_symlinks,
        exec_policy_engine: execution_config.exec_policy_engine.clone(),
        frozen_spec: None,
        catalog_cache: std::sync::Arc::new(std::sync::Mutex::new(
            crate::config_persistence::load_catalog_cache(),
        )),
        extra_tools: crate::core::engine::engine_config::ExtraTools(json_schema_terminator),
        batch_mode: false,
        task_budget_tokens: std::env::var("MIMOFAN_UNATTENDED_TASK_BUDGET")
            .ok()
            .and_then(|v| v.trim().parse::<usize>().ok()),
        resume_session: None,
        validation_retry: None,
        unattended,
        consolidation_interval_turns: std::env::var("MIMOFAN_CONSOLIDATION_INTERVAL_TURNS")
            .ok()
            .and_then(|v| v.trim().parse::<usize>().ok()),
        failure_log_path: std::env::var("MIMOFAN_UNATTENDED_FAILURE_LOG")
            .ok()
            .map(PathBuf::from),
        goal_self_check_after_compact: false,
        session_trace: execution_config.session_trace_config(),
    };

    let engine_handle = spawn_engine(engine_config, &execution_config);
    let mode = if auto_approve {
        AppMode::Yolo
    } else {
        AppMode::Agent
    };

    let mut loaded_session_id = None;
    if let Some(session_id) = resume_session_id.as_deref() {
        let manager = SessionManager::default_location()
            .context("could not open session manager for exec resume")?;
        let session_ref = crate::utils::redacted_identifier_for_log(session_id);
        let saved = manager
            .load_session_by_prefix(session_id)
            .with_context(|| format!("could not load session {session_ref}"))?;
        let saved_id = saved.metadata.id.clone();
        if saved.metadata.workspace != workspace && output_format == ExecOutputFormat::Text {
            eprintln!(
                "Warning: session {} was created in a different workspace ({}). Resuming anyway.",
                truncate_id(&saved_id),
                saved.metadata.workspace.display(),
            );
        }

        engine_handle
            .send(Op::SyncSession {
                session_id: Some(saved_id.clone()),
                messages: saved.messages,
                system_prompt: saved.system_prompt.map(SystemPrompt::Text),
                system_prompt_override: false,
                model: saved.metadata.model,
                workspace: saved.metadata.workspace,
            })
            .await?;
        loaded_session_id = Some(saved_id.clone());
        if output_format == ExecOutputFormat::Text && !json_output {
            eprintln!("{}", exec_resumed_session_line(&saved_id));
        }
    }

    engine_handle
        .send(Op::SendMessage {
            content: prompt.to_string(),
            mode,
            provider: Some(effective_provider),
            model: effective_model.clone(),
            goal_objective: None,
            goal_token_budget: None,
            goal_status: crate::tools::goal::GoalStatus::Active,
            allowed_tools: allowed_tools.clone(),
            dynamic_tools: Vec::new(),
            hook_executor: None,
            reasoning_effort: effective_reasoning_effort,
            reasoning_effort_auto: auto_model,
            response_format: None,
            auto_model,
            allow_shell: auto_approve || execution_config.allow_shell(),
            trust_mode,
            auto_approve,
            translation_enabled: false,
            show_thinking: settings.show_thinking,
            approval_mode: if auto_approve {
                crate::tui::approval::ApprovalMode::Auto
            } else {
                execution_config
                    .approval_policy
                    .as_deref()
                    .and_then(crate::tui::approval::ApprovalMode::from_config_value)
                    .unwrap_or_default()
            },
            verbosity: execution_config.verbosity.clone(),
            provenance: crate::core::ops::UserInputProvenance::ExternalUser,
        })
        .await?;

    #[derive(serde::Serialize)]
    struct ExecToolEntry {
        name: String,
        success: bool,
        output: String,
    }
    #[derive(serde::Serialize, Default)]
    struct ExecSummary {
        mode: String,
        model: String,
        prompt: String,
        output: String,
        tools: Vec<ExecToolEntry>,
        status: Option<String>,
        error: Option<String>,
    }
    let mut summary = ExecSummary {
        mode: "agent".to_string(),
        model: effective_model.clone(),
        prompt: prompt.to_string(),
        ..ExecSummary::default()
    };

    let should_persist_session =
        resume_session_id.is_some() || output_format == ExecOutputFormat::StreamJson;
    let mut latest_session_id = loaded_session_id;
    let mut latest_messages: Vec<Message> = Vec::new();
    let mut latest_system_prompt: Option<SystemPrompt> = None;
    let mut latest_model = effective_model;
    let mut latest_workspace = workspace.clone();

    let mut stdout = io::stdout();
    let mut ends_with_newline = false;
    loop {
        let event = {
            let mut rx = engine_handle.rx_event.write().await;
            rx.recv().await
        };

        let Some(event) = event else {
            break;
        };

        match event {
            Event::MessageDelta { content, .. } => {
                summary.output.push_str(&content);
                if output_format == ExecOutputFormat::StreamJson {
                    emit_exec_stream_event(&ExecStreamEvent::Content { content })?;
                } else if !json_output {
                    print!("{content}");
                    stdout.flush()?;
                }
                ends_with_newline = summary.output.ends_with('\n');
            }
            Event::MessageComplete { .. }
                if output_format == ExecOutputFormat::Text
                    && !json_output
                    && !ends_with_newline =>
            {
                println!();
            }
            Event::ThinkingDelta { .. } => {
                // Exec stream-json intentionally omits reasoning deltas; the
                // TUI transcript retains its existing Activity Detail surface.
            }
            Event::ToolCallStarted { id, name, input } => {
                if output_format == ExecOutputFormat::StreamJson {
                    emit_exec_stream_event(&ExecStreamEvent::ToolUse { name, id, input })?;
                } else if !json_output {
                    let summary = crate::tui::history::summarize_tool_args(&input);
                    if let Some(summary) = summary {
                        eprintln!("tool: {name} ({summary})");
                    } else {
                        eprintln!("tool: {name}");
                    }
                }
            }
            Event::ToolCallComplete {
                id, name, result, ..
            } => match result {
                Ok(output) => {
                    // `--json-schema` terminator (#824): when the model calls
                    // the synthetic terminator with a schema-valid submission,
                    // surface the submitted payload as the final output and end
                    // the run immediately.
                    if name == crate::tools::json_schema_terminator::JSON_SCHEMA_TERMINATOR_NAME
                        && output.success
                    {
                        let submitted = json_schema_submission.lock().unwrap().take();
                        if let Some(submitted) = submitted {
                            summary.output = serde_json::to_string_pretty(&submitted)
                                .unwrap_or_else(|_| submitted.to_string());
                            summary.status = Some("completed".to_string());
                            let _ = engine_handle.send(Op::Shutdown).await;
                            break;
                        }
                    }
                    summary.tools.push(ExecToolEntry {
                        name: name.clone(),
                        success: output.success,
                        output: output.content.clone(),
                    });
                    if output_format == ExecOutputFormat::StreamJson {
                        emit_exec_stream_event(&ExecStreamEvent::ToolResult {
                            id,
                            output: output.content,
                            status: if output.success {
                                "success".to_string()
                            } else {
                                "error".to_string()
                            },
                        })?;
                    } else if !json_output {
                        if name == "exec_shell" && !output.content.trim().is_empty() {
                            eprintln!("tool {name} completed");
                            eprintln!(
                                "--- stdout/stderr ---\n{}\n---------------------",
                                output.content
                            );
                        } else {
                            eprintln!(
                                "tool {name} completed: {}",
                                crate::tui::history::summarize_tool_output(&output.content)
                            );
                        }
                    }
                }
                Err(err) => {
                    let error_text = err.to_string();
                    summary.tools.push(ExecToolEntry {
                        name: name.clone(),
                        success: false,
                        output: error_text.clone(),
                    });
                    if output_format == ExecOutputFormat::StreamJson {
                        emit_exec_stream_event(&ExecStreamEvent::ToolResult {
                            id,
                            output: error_text,
                            status: "error".to_string(),
                        })?;
                    } else if !json_output {
                        eprintln!("tool {name} failed: {err}");
                    }
                }
            },
            Event::AgentSpawned { id, prompt, .. }
                if output_format == ExecOutputFormat::Text && !json_output =>
            {
                eprintln!(
                    "sub-agent {id} spawned: {}",
                    crate::tui::history::summarize_tool_output(&prompt)
                );
            }
            Event::AgentProgress { id, status, .. }
                if output_format == ExecOutputFormat::Text && !json_output =>
            {
                eprintln!("sub-agent {id}: {status}");
            }
            Event::AgentComplete { id, result }
                if output_format == ExecOutputFormat::Text && !json_output =>
            {
                eprintln!(
                    "sub-agent {id} completed: {}",
                    crate::tui::history::summarize_tool_output(&result)
                );
            }
            Event::AgentSpawned { .. }
            | Event::AgentProgress { .. }
            | Event::AgentComplete { .. } => {}
            Event::ApprovalRequired { id, .. } => {
                if auto_approve {
                    let _ = engine_handle.approve_tool_call(id).await;
                } else {
                    let _ = engine_handle.deny_tool_call(id).await;
                }
            }
            Event::ElevationRequired {
                tool_id,
                tool_name,
                denial_reason,
                ..
            } => {
                if auto_approve {
                    if output_format == ExecOutputFormat::Text && !json_output {
                        eprintln!("sandbox denied {tool_name}: {denial_reason} (auto-elevating)");
                    }
                    let policy = crate::sandbox::SandboxPolicy::DangerFullAccess;
                    let _ = engine_handle.retry_tool_with_policy(tool_id, policy).await;
                } else {
                    if output_format == ExecOutputFormat::Text && !json_output {
                        eprintln!("sandbox denied {tool_name}: {denial_reason}");
                    }
                    let _ = engine_handle.deny_tool_call(tool_id).await;
                }
            }
            Event::Error {
                envelope,
                recoverable: _,
            } => {
                summary.error = Some(envelope.message.clone());
                if output_format == ExecOutputFormat::StreamJson {
                    emit_exec_stream_event(&ExecStreamEvent::Error {
                        error: envelope.message,
                    })?;
                } else if !json_output {
                    eprintln!("error: {}", envelope.message);
                }
            }
            Event::TurnComplete {
                status,
                error,
                usage,
                ..
            } => {
                summary.status = Some(format!("{status:?}").to_lowercase());
                summary.error = error;
                let saved_session_id = if should_persist_session && !latest_messages.is_empty() {
                    match persist_exec_session(
                        &latest_messages,
                        &latest_model,
                        &latest_workspace,
                        &latest_system_prompt,
                        latest_session_id.as_deref(),
                        u64::from(usage.input_tokens) + u64::from(usage.output_tokens),
                    ) {
                        Ok(id) => {
                            if output_format == ExecOutputFormat::Text && !json_output {
                                eprintln!("{}", exec_saved_session_line(&id));
                            }
                            Some(id)
                        }
                        Err(err) => {
                            if output_format == ExecOutputFormat::Text && !json_output {
                                eprintln!("warning: failed to save exec session: {err}");
                            }
                            latest_session_id.clone()
                        }
                    }
                } else {
                    latest_session_id.clone()
                };

                if output_format == ExecOutputFormat::StreamJson {
                    if let Some(id) = saved_session_id.as_ref() {
                        emit_exec_stream_event(&ExecStreamEvent::SessionCapture {
                            content: exec_stream_session_ref(id),
                        })?;
                    }
                    emit_exec_stream_event(&ExecStreamEvent::Metadata {
                        meta: ExecStreamMeta {
                            model: latest_model.clone(),
                            input_tokens: usage.input_tokens,
                            output_tokens: usage.output_tokens,
                            resume_command: saved_session_id
                                .as_deref()
                                .map(exec_stream_resume_hint)
                                .unwrap_or_default(),
                            session_id: saved_session_id
                                .as_deref()
                                .map(exec_stream_session_ref)
                                .unwrap_or_default(),
                            workspace: latest_workspace.display().to_string(),
                            message_count: latest_messages.len(),
                            status: summary.status.clone(),
                        },
                    })?;
                    emit_exec_stream_event(&ExecStreamEvent::Done)?;
                }
                let _ = engine_handle.send(Op::Shutdown).await;
                break;
            }
            Event::SessionUpdated {
                session_id,
                messages,
                system_prompt,
                model,
                workspace,
            } => {
                latest_session_id = Some(session_id);
                latest_messages = messages;
                latest_system_prompt = system_prompt;
                latest_model = model;
                latest_workspace = workspace;
            }
            // #3027: surface the engine's max-steps notice in text mode so a
            // --max-turns run that stops early says why instead of going quiet.
            Event::Status { message }
                if output_format == ExecOutputFormat::Text
                    && !json_output
                    && message.contains("Reached maximum steps") =>
            {
                eprintln!("{message}");
            }
            _ => {}
        }
    }

    if json_output {
        println!("{}", serde_json::to_string_pretty(&summary)?);
    }

    if let Some(error) = summary.error.as_ref()
        && !error.trim().is_empty()
    {
        bail!("exec turn failed: {error}");
    }

    if matches!(
        summary.status.as_deref(),
        Some("failed" | "canceled" | "interrupted")
    ) {
        let status = summary.status.as_deref().unwrap_or("unknown");
        bail!("exec turn ended with status {status}");
    }

    Ok(())
}
