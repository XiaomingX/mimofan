//! engine config prompt 子系统（从 ui 上帝文件切片）
use super::*;

pub(crate) fn build_engine_config(app: &App, config: &Config) -> EngineConfig {
    let provider = app.api_provider;
    let max_subagents = app.max_subagents.clamp(1, crate::config::MAX_SUBAGENTS);
    EngineConfig {
        model: app.model.clone(),
        active_route_limits: app.active_route_limits,
        workspace: app.workspace.clone(),
        allow_shell: app.allow_shell,
        trust_mode: app.trust_mode,
        notes_path: config.notes_path(),
        mcp_config_path: config.mcp_config_path(),
        skills_dir: app.skills_dir.clone(),
        skills_scan_mimofan_only: app.skills_scan_mimofan_only,
        instructions: configured_instruction_sources(config),
        project_context_pack_enabled: config.project_context_pack_enabled(),
        translation_enabled: app.translation_enabled,
        show_thinking: app.show_thinking,
        verbosity: app.verbosity.clone(),
        // Effectively unlimited. V4 has a 1M context window and the user
        // wants the model running until it's actually done. The previous cap
        // of 100 hit the ceiling on long multi-step plans (wide refactors,
        // sub-agent orchestration) and presented as the agent "giving up
        // mid-task". `u32::MAX` is the type ceiling; users can still
        // interrupt with Ctrl+C / Esc, and a turn naturally ends when the
        // model stops emitting tool calls. A real runaway is rare and
        // human-noticeable; we trust the operator over a hard step cap.
        max_steps: u32::MAX,
        max_subagents,
        max_admitted_subagents: config
            .max_admitted_subagents_for_provider(provider)
            .max(max_subagents),
        launch_concurrency: config.launch_concurrency_for_provider(provider),
        subagents_enabled: config.subagents_enabled_for_provider(provider),
        features: config.features(),
        auto_review_policy: config.auto_review_policy(),
        compaction: app.compaction_config(),
        todos: app.todos.clone(),
        plan_state: app.plan_state.clone(),
        goal_queue: crate::tools::goal::new_shared_goal_queue_from_host_status(
            app.hunt.quarry.clone(),
            app.hunt.token_budget,
            app.hunt.verdict.goal_status(),
        ),
        max_spawn_depth: config.subagent_max_spawn_depth_for_provider(provider),
        subagent_token_budget: config.subagent_token_budget_for_provider(provider),
        allowed_tools: app.active_allowed_tools.clone(),
        disallowed_tools: None,
        hook_executor: app.runtime_services.hook_executor.clone(),
        network_policy: config.network.clone().map(|toml_cfg| {
            crate::network_policy::NetworkPolicyDecider::with_default_audit(toml_cfg.into_runtime())
        }),
        snapshots_enabled: config.snapshots_config().enabled,
        snapshots_max_workspace_bytes: config
            .snapshots_config()
            .max_workspace_gb
            .saturating_mul(1024 * 1024 * 1024),
        lsp_config: config
            .lsp
            .clone()
            .map(crate::config::LspConfigToml::into_runtime),
        runtime_services: app.runtime_services.clone(),
        subagent_model_overrides: config.subagent_model_overrides(),
        subagent_api_timeout: Duration::from_secs(
            config.subagent_api_timeout_secs_for_provider(provider),
        ),
        stream_chunk_timeout: Duration::from_secs(app.stream_chunk_timeout_secs),
        subagent_heartbeat_timeout: Duration::from_secs(
            config.subagent_heartbeat_timeout_secs_for_provider(provider),
        ),
        prefer_bwrap: config.prefer_bwrap.unwrap_or(false),
        memory_enabled: config.memory_enabled(),
        memory_dir: config.memory_dir(),
        speech_output_dir: config.speech_output_dir(),
        vision_config: config.vision_model_config(),
        strict_tool_mode: config.strict_tool_mode.unwrap_or(false),
        goal_objective: app.hunt.quarry.clone(),
        goal_token_budget: app.hunt.token_budget,
        goal_status: app.hunt.verdict.goal_status(),
        locale_tag: app.ui_locale.tag().to_string(),
        workshop: config.workshop.clone(),
        search_provider: config.search_provider(),
        search_api_key: config.search.as_ref().and_then(|s| s.api_key.clone()),
        search_base_url: config.search.as_ref().and_then(|s| s.base_url.clone()),
        tools_always_load: config.tools_always_load(),
        tools: config.tools.clone(),
        workspace_follow_symlinks: app.workspace_follow_symlinks,
        exec_policy_engine: config.exec_policy_engine.clone(),
        frozen_spec: app.frozen_spec.clone(),
        catalog_cache: app.catalog_cache.clone(),
    }
}

fn configured_instruction_sources(config: &Config) -> Vec<prompts::InstructionSource> {
    config
        .instructions_paths()
        .into_iter()
        .map(Into::into)
        .collect()
}

pub(crate) fn build_app_system_prompt(app: &App, config: &Config) -> SystemPrompt {
    let instructions = configured_instruction_sources(config);
    prompts::system_prompt_for_mode_with_context_skills_and_session(
        &app.workspace,
        None,
        None,
        Some(&instructions),
        prompts::PromptSessionContext {
            user_memory_block: None,
            goal_objective: app.hunt.quarry.as_deref(),
            goal_completion_check: None,
            goal_progress_checklist: None,
            project_context_pack_enabled: config.project_context_pack_enabled(),
            locale_tag: app.ui_locale.tag(),
            translation_enabled: app.translation_enabled,
            model_id: &app.model,
            context_window_override: Some(
                provider_capability(app.api_provider, &app.model).context_window,
            ),
            show_thinking: app.show_thinking,
            verbosity: app.verbosity.as_deref(),
            skills_scan_mimofan_only: app.skills_scan_mimofan_only,
            frozen_spec: app.frozen_spec.as_deref(),
        },
    )
}
