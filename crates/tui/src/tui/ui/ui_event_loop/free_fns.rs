//! Free helper functions formerly at the tail of `ui_event_loop`.
//!
//! Extracted during the #647 safe-migration split to shrink the 4300-line
//! `ui_event_loop.rs`. They depend on the parent `ui` module's helpers, so
//! they pull them in via `use crate::tui::ui::*` (mirrors the old
//! `use super::*` that the single file relied on).

use crate::tui::ui::*;

pub(crate) fn goal_status_from_snapshot(snapshot: &GoalSnapshot) -> Option<GoalStatus> {
    match snapshot.status.trim() {
        "active" => Some(GoalStatus::Active),
        "paused" => Some(GoalStatus::Paused),
        "complete" => Some(GoalStatus::Complete),
        "blocked" => Some(GoalStatus::Blocked),
        _ => None,
    }
}

pub(crate) fn apply_goal_snapshot_to_app(app: &mut App, snapshot: &GoalSnapshot) -> bool {
    let Some(objective) = snapshot
        .objective
        .as_deref()
        .map(str::trim)
        .filter(|objective| !objective.is_empty())
    else {
        return false;
    };
    let Some(status) = goal_status_from_snapshot(snapshot) else {
        tracing::warn!("ignoring unknown runtime goal status: {}", snapshot.status);
        return false;
    };
    let verdict = HuntVerdict::from_goal_status(status);
    let objective_changed = app.hunt.quarry.as_deref() != Some(objective);
    let changed = objective_changed
        || app.hunt.token_budget != snapshot.token_budget
        || app.hunt.tokens_used != snapshot.tokens_used
        || app.hunt.time_used_seconds != snapshot.time_used_seconds
        || app.hunt.continuation_count != snapshot.continuation_count
        || app.hunt.verdict != verdict;
    if !changed {
        return false;
    }

    app.hunt.quarry = Some(objective.to_string());
    app.hunt.token_budget = snapshot.token_budget;
    app.hunt.tokens_used = snapshot.tokens_used;
    app.hunt.time_used_seconds = snapshot.time_used_seconds;
    app.hunt.continuation_count = snapshot.continuation_count;
    app.hunt.verdict = verdict;
    if objective_changed || app.hunt.started_at.is_none() {
        app.hunt.started_at = Some(Instant::now());
    }
    true
}

// sync_mode_update, apply_mode_update moved to provider_and_model.rs

pub(crate) async fn handle_bang_shell_input(
    app: &mut App,
    engine_handle: &EngineHandle,
    input: &str,
) -> Result<bool> {
    let command = match shell_command_from_bang_input(input) {
        Ok(Some(command)) => command,
        Ok(None) => return Ok(false),
        Err(message) => {
            app.status_message = Some(format!("Error: {message}"));
            return Ok(true);
        }
    };

    engine_handle
        .send(Op::RunShellCommand {
            command: command.to_string(),
            mode: app.mode,
            trust_mode: app.trust_mode,
            auto_approve: app.mode == AppMode::Yolo,
            approval_mode: app.approval_mode,
        })
        .await?;
    app.status_message = Some(format!("Shell command submitted: {command}"));
    Ok(true)
}

pub(crate) fn is_model_visible_tool_call(id: &str) -> bool {
    !id.starts_with(USER_SHELL_TOOL_ID_PREFIX)
}

// apply_model_and_compaction_update moved to provider_and_model.rs

pub(crate) async fn drain_web_config_events(
    web_config_session: &mut Option<WebConfigSession>,
    app: &mut App,
    config: &mut Config,
    engine_handle: &EngineHandle,
) -> bool {
    let Some(session) = web_config_session.as_mut() else {
        return true;
    };

    let mut keep_session = true;
    while let Ok(event) = session.receiver.try_recv() {
        match event {
            WebConfigSessionEvent::Draft(doc) => {
                match config_ui::apply_document(doc, app, config, false) {
                    Ok(outcome) if outcome.changed => {
                        if outcome.requires_engine_sync {
                            apply_model_and_compaction_update(
                                engine_handle,
                                app.compaction_config(),
                                app.mode,
                                app.active_route_limits,
                            )
                            .await;
                        }
                        app.status_message = Some(format!(
                            "Web config draft applied: {}",
                            outcome.final_message
                        ));
                    }
                    Ok(_) => {}
                    Err(err) => {
                        app.add_message(HistoryCell::System {
                            content: format!("Web config draft apply failed: {err}"),
                        });
                    }
                }
            }
            WebConfigSessionEvent::Committed(doc) => {
                keep_session = false;
                match config_ui::apply_document(doc, app, config, true) {
                    Ok(outcome) => {
                        if outcome.requires_engine_sync {
                            apply_model_and_compaction_update(
                                engine_handle,
                                app.compaction_config(),
                                app.mode,
                                app.active_route_limits,
                            )
                            .await;
                        }
                        app.add_message(HistoryCell::System {
                            content: outcome.final_message.clone(),
                        });
                        app.status_message = Some(outcome.final_message);
                    }
                    Err(err) => {
                        app.add_message(HistoryCell::System {
                            content: format!("Web config commit failed: {err}"),
                        });
                    }
                }
            }
            WebConfigSessionEvent::Failed(err) => {
                keep_session = false;
                app.add_message(HistoryCell::System {
                    content: format!("Web config session failed: {err}"),
                });
            }
        }
    }

    keep_session
}

// apply_model_picker_choice, apply_picker_effort_choice, switch_provider,
// apply_provider_fallback_switch, display_base_url_host,
// sync_config_provider_from_app, provider_picker_model_override
// moved to provider_and_model.rs

pub(crate) fn open_text_pager(app: &mut App, title: String, content: String) {
    let width = app
        .viewport
        .last_transcript_area
        .map(|area| area.width)
        .unwrap_or(80);
    app.view_stack.push(PagerView::from_text(
        title,
        &content,
        width.saturating_sub(2),
    ));
}

pub(crate) fn open_context_inspector(app: &mut App) {
    let width = app
        .viewport
        .last_transcript_area
        .map(|area| area.width)
        .unwrap_or(80);
    let content = build_context_inspector_text(app, app.ui_locale);
    app.view_stack.push(PagerView::from_text(
        tr(app.ui_locale, MessageId::CtxInspTitle),
        &content,
        width.saturating_sub(2),
    ));
}

// File-picker relevance scoring moved to `tui/file_picker_relevance.rs`.

pub(crate) async fn apply_command_result(
    terminal: &mut AppTerminal,
    app: &mut App,
    engine_handle: &mut EngineHandle,
    task_manager: &SharedTaskManager,
    config: &mut Config,
    #[cfg_attr(not(feature = "web"), allow(unused_variables))] web_config_session: &mut Option<
        WebConfigSession,
    >,
    result: commands::CommandResult,
) -> Result<bool> {
    if let Some(msg) = result.message {
        app.add_message(HistoryCell::System { content: msg });
    }

    if let Some(action) = result.action {
        match action {
            AppAction::Quit => {
                let _ = engine_handle.send(Op::Shutdown).await;
                return Ok(true);
            }
            AppAction::SaveSession(path) => {
                app.status_message = Some(format!("Session saved to {}", path.display()));
            }
            AppAction::LoadSession(path) => {
                app.status_message = Some(format!("Session loaded from {}", path.display()));
            }
            AppAction::SyncSession {
                session_id,
                messages,
                system_prompt,
                model,
                workspace,
            } => {
                let mut session_id = session_id;
                let is_full_reset = messages.is_empty() && system_prompt.is_none();
                if is_full_reset && session_id.is_none() {
                    let new_session_id = uuid::Uuid::new_v4().to_string();
                    app.current_session_id = Some(new_session_id.clone());
                    session_id = Some(new_session_id);
                }
                let _ = engine_handle
                    .send(Op::SyncSession {
                        session_id,
                        messages,
                        system_prompt,
                        system_prompt_override: false,
                        model,
                        workspace,
                    })
                    .await;
                let _ = engine_handle
                    .send(Op::SetCompaction {
                        config: app.compaction_config(),
                    })
                    .await;
                if is_full_reset {
                    if let Ok(manager) = SessionManager::default_location() {
                        let session = build_session_snapshot(app, &manager);
                        app.current_session_id = Some(session.metadata.id.clone());
                        persistence_actor::persist(PersistRequest::SessionSnapshot(
                            session.clone(),
                        ));
                        persistence_actor::persist_plan_state(
                            session.metadata.id.clone(),
                            app.current_plan_and_todo(),
                        );
                    }
                    persistence_actor::persist(PersistRequest::ClearCheckpoint);
                }
            }
            AppAction::ModeChanged(mode) => {
                sync_mode_update(engine_handle, mode).await;
            }
            AppAction::SpecFrozen => {
                // Spec freeze state is already set in the command handler.
                // The frozen spec will be injected into the system prompt
                // when the next message is sent.
                app.status_message =
                    Some("Spec frozen. Agent will respect the frozen spec.".to_string());
            }
            AppAction::SpecUnfrozen => {
                // Spec unfreeze state is already set in the command handler.
                app.status_message =
                    Some("Spec unfrozen. Agent is no longer constrained.".to_string());
            }
            AppAction::SendMessage(content) => {
                let queued = build_queued_message(app, content);
                submit_or_steer_message(app, config, engine_handle, queued).await?;
            }
            AppAction::SetGoalStatus {
                status,
                clear,
                loop_config,
            } => {
                let _ = engine_handle
                    .send(Op::SetGoalStatus {
                        status,
                        clear,
                        loop_config,
                    })
                    .await;
            }
            AppAction::VoiceCapture => {
                use commands::voice::VoiceCaptureOutcome;
                match commands::voice::capture_and_transcribe(app, config).await {
                    Ok(VoiceCaptureOutcome::Insert(text)) => {
                        app.insert_str(&text);
                        app.status_message = Some(format!(
                            "{}: {text}",
                            tr(app.ui_locale, MessageId::VoiceTranscribed)
                        ));
                    }
                    Ok(VoiceCaptureOutcome::Send(content)) => {
                        app.status_message =
                            Some(tr(app.ui_locale, MessageId::VoiceTranscribed).to_string());
                        let queued = build_queued_message(app, content);
                        submit_or_steer_message(app, config, engine_handle, queued).await?;
                    }
                    Err(err) => {
                        app.voice_enabled = false;
                        app.status_message = Some(err);
                    }
                }
            }
            AppAction::ListSubAgents => {
                let _ = engine_handle.send(Op::ListSubAgents).await;
            }
            AppAction::FetchModels => {
                app.status_message = Some("Fetching models...".to_string());
                match fetch_available_models(config, app.catalog_cache.clone()).await {
                    Ok(models) => {
                        app.add_message(HistoryCell::System {
                            content: format_helpers::available_models_message(&app.model, &models),
                        });
                        app.status_message = Some(format!("Found {} model(s)", models.len()));
                    }
                    Err(error) => {
                        app.add_message(HistoryCell::System {
                            content: format!(
                                "Failed to fetch models from {}: {error}",
                                config.api_provider().display_name()
                            ),
                        });
                    }
                }
            }
            AppAction::CacheWarmup => {
                app.status_message = Some("Warming DeepSeek cache...".to_string());
                match run_cache_warmup(app, config).await {
                    Ok((usage, base_url, inspection)) => {
                        app.session.last_base_url = Some(base_url.clone());
                        app.session.last_warmup_key = Some(CacheWarmupKey::from_inspection(
                            &format!("{:?}", app.api_provider),
                            &app.model,
                            &base_url,
                            &inspection,
                        ));
                        let mut message = format_helpers::cache_warmup_result(&usage);
                        if let Some(key) = app.session.last_warmup_key.as_ref() {
                            message.push_str(&format!("\nWarmup key: {}", key.hash_short()));
                        }
                        // Append prefix-cache stability info.
                        if app.prefix_checks_total > 0 {
                            let changes = app.prefix_change_count;
                            let total = app.prefix_checks_total;
                            let stable = total.saturating_sub(changes);
                            let pct = app
                                .prefix_stability_pct
                                .map(|p| format!("{p}%"))
                                .unwrap_or_else(|| "--".to_string());
                            message.push_str(&format!(
                                "\n\nPrefix stability: {pct} ({stable}/{total} checks stable, {changes} change{})",
                                if changes == 1 { "" } else { "s" }
                            ));
                            if let Some(ref desc) = app.last_prefix_change_desc {
                                message.push_str(&format!("\nLast prefix change: {desc}"));
                            }
                        }
                        app.add_message(HistoryCell::System { content: message });
                        app.status_message = Some("Cache warmup complete".to_string());
                    }
                    Err(error) => {
                        app.add_message(HistoryCell::System {
                            content: format!("Cache warmup failed: {error}"),
                        });
                        app.status_message = Some("Cache warmup failed".to_string());
                    }
                }
            }
            AppAction::SwitchProvider { provider, model } => {
                switch_provider(app, engine_handle, config, provider, model).await;
                // Refresh balance after provider switch.
                let balance_cooldown_expired = app
                    .last_balance_fetch
                    .is_none_or(|t| t.elapsed() >= BALANCE_FETCH_COOLDOWN);
                if balance_cooldown_expired && should_fetch_deepseek_balance(app) {
                    let cell = app.balance_cell.clone();
                    let api_key = config.api_key().unwrap_or_default();
                    let base_url = config.api_base_url();
                    if !api_key.is_empty() {
                        app.last_balance_fetch = Some(Instant::now());
                        tokio::spawn(async move {
                            let provider = ReqwestBalanceProvider::new();
                            if let Some(info) =
                                fetch_deepseek_balance(&provider, &api_key, &base_url).await
                                && let Ok(mut guard) = cell.lock()
                            {
                                *guard = Some(info);
                            }
                        });
                    }
                } else {
                    // Clear balance when switching to a non-DeepSeek provider.
                    if let Ok(mut guard) = app.balance_cell.lock() {
                        *guard = None;
                    }
                }
            }
            AppAction::UpdateCompaction(compaction) => {
                apply_model_and_compaction_update(
                    engine_handle,
                    compaction,
                    app.mode,
                    app.active_route_limits,
                )
                .await;
            }
            AppAction::UpdateStreamChunkTimeout(timeout_secs) => {
                let _ = engine_handle
                    .send(Op::SetStreamChunkTimeout { timeout_secs })
                    .await;
            }
            AppAction::UpdateSubagentRuntimeConfig {
                enabled,
                max_subagents,
                launch_concurrency,
                max_spawn_depth,
                api_timeout_secs,
                heartbeat_timeout_secs,
            } => {
                let _ = engine_handle
                    .send(Op::SetSubagentRuntimeConfig {
                        enabled,
                        max_subagents,
                        launch_concurrency,
                        max_spawn_depth,
                        api_timeout_secs,
                        heartbeat_timeout_secs,
                    })
                    .await;
            }
            AppAction::OpenConfigEditor(mode) => match mode {
                ConfigUiMode::Native => {
                    if app.view_stack.top_kind() != Some(ModalKind::Config) {
                        app.view_stack.push(ConfigView::new_for_app(app));
                    }
                }
                ConfigUiMode::Tui => {
                    pause_terminal(
                        terminal,
                        app.use_alt_screen,
                        app.use_mouse_capture,
                        app.use_bracketed_paste,
                    )?;
                    let editor_result = config_ui::run_tui_editor(app, config)
                        .and_then(|doc| config_ui::apply_document(doc, app, config, true));
                    resume_terminal(
                        terminal,
                        app.use_alt_screen,
                        app.use_mouse_capture,
                        app.use_bracketed_paste,
                        app.synchronized_output_enabled,
                    )?;
                    match editor_result {
                        Ok(outcome) => {
                            if outcome.requires_engine_sync {
                                apply_model_and_compaction_update(
                                    engine_handle,
                                    app.compaction_config(),
                                    app.mode,
                                    app.active_route_limits,
                                )
                                .await;
                            }
                            app.add_message(HistoryCell::System {
                                content: outcome.final_message.clone(),
                            });
                            app.status_message = Some(outcome.final_message);
                        }
                        Err(err) => {
                            app.add_message(HistoryCell::System {
                                content: format!("Config UI failed: {err}"),
                            });
                        }
                    }
                }
                ConfigUiMode::Web => {
                    #[cfg(feature = "web")]
                    {
                        let session = config_ui::start_web_editor(app, config).await?;
                        let url = format!("http://{}", session.addr);
                        let open_err = config_ui::open_browser(&url).err();
                        if let Some(err) = open_err {
                            app.add_message(HistoryCell::System {
                                content: format!("Failed to open browser automatically: {err}"),
                            });
                        }
                        app.status_message = Some(format!("web ui listen on: {url}"));
                        *web_config_session = Some(session);
                    }
                    #[cfg(not(feature = "web"))]
                    {
                        app.add_message(HistoryCell::System {
                            content: "This build does not include the web config UI.".to_string(),
                        });
                    }
                }
            },
            AppAction::OpenConfigView => {
                if app.view_stack.top_kind() != Some(ModalKind::Config) {
                    app.view_stack.push(ConfigView::new_for_app(app));
                }
            }
            AppAction::OpenModelPicker => {
                if app.view_stack.top_kind() != Some(ModalKind::ModelPicker) {
                    app.view_stack
                        .push(crate::tui::model_picker::ModelPickerView::new(app));
                }
            }
            AppAction::OpenProviderPicker => {
                if app.view_stack.top_kind() != Some(ModalKind::ProviderPicker) {
                    app.view_stack
                        .push(crate::tui::provider_picker::ProviderPickerView::new(
                            app.api_provider,
                            config,
                            &app.catalog_cache.lock().expect("catalog cache poisoned"),
                        ));
                }
            }
            AppAction::OpenModePicker => {
                if app.view_stack.top_kind() != Some(ModalKind::ModePicker) {
                    app.view_stack
                        .push(crate::tui::views::mode_picker::ModePickerView::new(
                            app.mode,
                            app.ui_locale,
                        ));
                }
            }
            AppAction::OpenBacktrackOverlay => {
                open_backtrack_overlay(app);
            }
            AppAction::OpenStatusPicker => {
                if app.view_stack.top_kind() != Some(ModalKind::StatusPicker) {
                    app.view_stack
                        .push(crate::tui::views::status_picker::StatusPickerView::new(
                            &app.status_items,
                            app.api_provider,
                            app.ui_locale,
                        ));
                }
            }
            AppAction::OpenFleetSetup => {
                if app.view_stack.top_kind() != Some(ModalKind::FleetSetup) {
                    app.view_stack
                        .push(crate::tui::views::fleet_setup::FleetSetupView::new(
                            app, config,
                        ));
                }
            }
            AppAction::OpenExternalUrl { url, label } => match open_external_url(&url) {
                Ok(()) => {
                    app.status_message = Some(format!("Opened {label} in your browser"));
                }
                Err(err) => {
                    app.add_message(HistoryCell::System {
                        content: format!(
                            "Could not open {label} automatically: {err}\n\nThe URL is printed above."
                        ),
                    });
                }
            },
            AppAction::OpenContextInspector => {
                open_context_inspector(app);
            }
            AppAction::CompactContext { instructions } => {
                app.status_message = Some(match instructions.as_deref() {
                    Some(text) => format!("Compacting context (focus: {text})..."),
                    None => "Compacting context...".to_string(),
                });
                let _ = engine_handle
                    .send(Op::CompactContext { instructions })
                    .await;
            }
            AppAction::PurgeContext => {
                app.status_message = Some("Agent purging context...".to_string());
                let _ = engine_handle.send(Op::PurgeContext).await;
            }
            AppAction::TaskAdd { prompt } => {
                let request = NewTaskRequest {
                    prompt: prompt.clone(),
                    model: Some(app.model.clone()),
                    workspace: Some(app.workspace.clone()),
                    mode: Some(task_mode_label(app.mode).to_string()),
                    allow_shell: Some(app.allow_shell),
                    trust_mode: Some(app.trust_mode),
                    auto_approve: Some(app.approval_mode == ApprovalMode::Auto),
                };
                match task_manager.add_task(request).await {
                    Ok(task) => {
                        app.add_message(HistoryCell::System {
                            content: format!(
                                "Task queued: {} ({})",
                                task.id,
                                summarize_tool_output(&task.prompt)
                            ),
                        });
                        app.status_message = Some(format!("Queued {}", task.id));
                    }
                    Err(err) => {
                        app.add_message(HistoryCell::System {
                            content: format!("Failed to queue task: {err}"),
                        });
                    }
                }
                refresh_active_task_panel(app, task_manager).await;
            }
            AppAction::TaskList => {
                let tasks = task_manager.list_tasks(Some(30)).await;
                refresh_active_task_panel(app, task_manager).await;
                app.add_message(HistoryCell::System {
                    content: format_task_list(&tasks),
                });
            }
            AppAction::TaskShow { id } => match task_manager.get_task(&id).await {
                Ok(task) => open_task_pager(app, &task),
                Err(err) => {
                    app.add_message(HistoryCell::System {
                        content: format!("Task lookup failed: {err}"),
                    });
                }
            },
            AppAction::TaskCancel { id } => {
                match task_manager.cancel_task(&id).await {
                    Ok(task) => {
                        app.add_message(HistoryCell::System {
                            content: format!("Task {} status: {:?}", task.id, task.status),
                        });
                    }
                    Err(err) => {
                        app.add_message(HistoryCell::System {
                            content: format!("Task cancel failed: {err}"),
                        });
                    }
                }
                refresh_active_task_panel(app, task_manager).await;
            }
            AppAction::ShellJob(action) => {
                handle_shell_job_action(app, action);
                // Immediately sync the task panel after cancel/poll so the
                // Tasks sidebar stays accurate without waiting for the
                // next 2.5 s periodic refresh (#2937).
                refresh_active_task_panel(app, task_manager).await;
            }
            AppAction::Mcp(action) => {
                handle_mcp_ui_action(app, config, action).await;
            }
            AppAction::SwitchWorkspace { workspace } => {
                switch_workspace(app, engine_handle, task_manager, config, workspace).await;
            }
            AppAction::SwitchProfile { profile } => {
                app.config_profile = Some(profile.clone());
                match Config::load(app.config_path.clone(), Some(&profile)) {
                    Ok(new_config) => {
                        *config = new_config.clone();
                        app.api_provider = config.api_provider();
                        let new_model = config.default_model();
                        app.set_model_selection(new_model.clone());
                        app.active_route_limits = None;
                        app.update_model_compaction_budget();
                        app.session.last_prompt_tokens = None;
                        app.session.last_completion_tokens = None;
                        app.session.last_output_throughput = None;
                        // Rebuild the engine with the new config so API key/model/base URL take effect.
                        let _ = engine_handle.send(Op::Shutdown).await;
                        let engine_config = build_engine_config(app, config);
                        *engine_handle = spawn_engine(engine_config, config);
                        if !app.api_messages.is_empty() {
                            let _ = engine_handle
                                .send(Op::SyncSession {
                                    session_id: app.current_session_id.clone(),
                                    messages: app.api_messages.clone(),
                                    system_prompt: app.system_prompt.clone(),
                                    system_prompt_override: false,
                                    model: app.model.clone(),
                                    workspace: app.workspace.clone(),
                                })
                                .await;
                        }
                        app.add_message(HistoryCell::System {
                            content: format!(
                                "Switched to profile '{profile}'. Model: {new_model}, Provider: {}",
                                config.api_provider().as_str()
                            ),
                        });
                        app.status_message = Some(format!("Profile: {profile}"));
                    }
                    Err(err) => {
                        app.config_profile = None;
                        app.status_message =
                            Some(format!("Failed to switch to profile '{profile}': {err}"));
                    }
                }
            }
            AppAction::ShareSession {
                history_len: _,
                model,
                mode,
                local,
            } => {
                let status = if app.api_messages.is_empty() {
                    "No session content to share.".to_string()
                } else {
                    let history_json = serde_json::to_string_pretty(&app.api_messages)
                        .unwrap_or_else(|_| "[]".to_string());
                    match crate::commands::share::perform_share(&history_json, &model, &mode, local)
                        .await
                    {
                        Ok(url) if local => format!("Session exported to local file: {url}"),
                        Ok(url) => format!("Session shared! URL: {url}"),
                        Err(err) => format!("Share failed: {err}"),
                    }
                };
                app.add_message(HistoryCell::System {
                    content: status.clone(),
                });
                app.status_message = Some(status);
            }
        }
    }

    Ok(false)
}

pub(crate) fn open_external_url(url: &str) -> Result<()> {
    crate::utils::open_url(url)
}

// apply_workspace_runtime_state, sync_runtime_workspace_state,
// switch_workspace moved to workspace.rs

// handle_mcp_ui_action, mcp_ui_action_refreshes_discovery,
// handle_shell_job_action moved to mcp_shell.rs

pub(crate) async fn execute_command_input(
    terminal: &mut AppTerminal,
    app: &mut App,
    engine_handle: &mut EngineHandle,
    task_manager: &SharedTaskManager,
    config: &mut Config,
    web_config_session: &mut Option<WebConfigSession>,
    input: &str,
) -> Result<bool> {
    if let Some(parsed_index) = parse_queue_send_command(input) {
        match parsed_index {
            Ok(index) => {
                send_queued_message_at_index_now(app, config, engine_handle, index).await?;
            }
            Err(message) => {
                app.status_message = Some(message);
            }
        }
        return Ok(false);
    }

    let result = commands::execute(input, app);
    // After /logout: clear the in-memory api_key fields so the next
    // onboarding round entering a new key doesn't see the stale value
    // (#343). The on-disk side is handled by clear_api_key() inside
    // commands::config::logout.
    if input.trim().eq_ignore_ascii_case("/logout") {
        // Only clear the active provider's in-memory API key, not every
        // provider.  The on-disk clear_api_key() inside commands::config::logout
        // already removes all saved keys; clearing only the active slot here
        // prevents surprising side-effects when the user has multiple providers
        // configured.
        config.api_key = None;
        config.provider_config_for_mut(app.api_provider).api_key = None;
        app.api_key_env_only = crate::config::active_provider_uses_env_only_api_key(config);
    }
    apply_command_result(
        terminal,
        app,
        engine_handle,
        task_manager,
        config,
        web_config_session,
        result,
    )
    .await
}

pub(crate) fn parse_queue_send_command(input: &str) -> Option<Result<usize, String>> {
    let rest = strip_queue_command_prefix(input.trim())?;
    let mut parts = rest.split_whitespace();
    let action = parts.next()?;
    if !matches!(action.to_ascii_lowercase().as_str(), "send" | "now") {
        return None;
    }
    let Some(raw_index) = parts.next() else {
        return Some(Err("Usage: /queue send <n>".to_string()));
    };
    if parts.next().is_some() {
        return Some(Err("Usage: /queue send <n>".to_string()));
    }
    let Ok(index) = raw_index.parse::<usize>() else {
        return Some(Err("Index must be a positive number".to_string()));
    };
    if index == 0 {
        return Some(Err("Index must be >= 1".to_string()));
    }
    Some(Ok(index - 1))
}

pub(crate) fn strip_queue_command_prefix(input: &str) -> Option<&str> {
    for prefix in ["/queue", "/queued"] {
        if let Some(rest) = input.strip_prefix(prefix)
            && (rest.is_empty() || rest.chars().next().is_some_and(char::is_whitespace))
        {
            return Some(rest);
        }
    }
    None
}
