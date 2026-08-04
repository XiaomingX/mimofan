//! View event handling, plan choices, backtrack, and overlay management.

use anyhow::Result;
use crossterm::event::{KeyCode, KeyModifiers};

use crate::config::{ApiProvider, Config};
use crate::config_ui::WebConfigSession;
use crate::core::engine::EngineHandle;
use crate::core::ops::Op;
use crate::session_manager::SessionManager;
use crate::task_manager::SharedTaskManager;

use super::super::app::{App, AppMode, QueuedMessage};
use super::super::history::HistoryCell;
use super::super::views::{ModalKind, ViewEvent};
use super::approval::{ApprovalDecisionEvent, apply_approval_decision};
use super::message_dispatch::dispatch_user_message;
use super::plan_choice::{PlanChoice, parse_plan_choice, plan_choice_from_option};
use super::provider_and_model::{
    apply_mode_update, apply_model_picker_choice, provider_picker_model_override, switch_provider,
    sync_mode_update,
};
use super::provider_picker_api_key::{
    apply_provider_picker_api_key, apply_provider_picker_auth_mode,
};
use super::session_loading::apply_loaded_session;
use super::workspace::sync_runtime_workspace_state;
use crate::tui::live_transcript::LiveTranscriptOverlay;
use crate::tui::widgets::pending_input_preview::{ContextPreviewItem, PendingInputPreview};

/// Build the pending-input preview widget from current `App` state.
///
/// v0.6.6 (#122) wires all three buckets:
/// - `pending_steers` — typed during a running turn + Esc; held until the
///   abort lands and gets resubmitted as a fresh merged turn.
/// - `rejected_steers` — engine declined a mid-turn steer (scaffolding;
///   no engine path produces these yet but the bucket renders with a distinct
///   rejected-steer label).
/// - `queued_messages` — Enter while busy (offline-mode FIFO); drained at
///   end-of-turn.
pub(crate) fn build_pending_input_preview(app: &App) -> PendingInputPreview {
    let mut preview = PendingInputPreview::new();
    let selected_attachment = app.selected_composer_attachment_index();
    let mut attachment_index = 0usize;
    preview.context_items = crate::tui::file_mention::pending_context_previews(
        &app.input,
        &app.workspace,
        std::env::current_dir().ok(),
    )
    .into_iter()
    .map(|item| {
        let selected = if item.removable {
            let selected = selected_attachment == Some(attachment_index);
            attachment_index += 1;
            selected
        } else {
            false
        };
        ContextPreviewItem {
            kind: item.kind,
            label: item.label,
            detail: item.detail,
            included: item.included,
            removable: item.removable,
            selected,
        }
    })
    .collect();
    preview.pending_steers = app
        .pending_steers
        .iter()
        .map(|m| m.display.clone())
        .collect();
    preview.rejected_steers = app.rejected_steers.iter().cloned().collect();
    preview.queued_messages = app
        .queued_messages
        .iter()
        .map(|m| m.display.clone())
        .collect();
    preview.editing_queued_message = app.queued_draft.as_ref().map(|draft| {
        if app.input.trim().is_empty() {
            draft.display.clone()
        } else {
            app.input.clone()
        }
    });
    preview
}

/// Refresh the live transcript overlay with current app state.
pub(crate) fn refresh_live_transcript_overlay(app: &mut App) {
    let Some(mut overlay) = app.view_stack.pop() else {
        return;
    };
    if let Some(typed) = overlay.as_any_mut().downcast_mut::<LiveTranscriptOverlay>() {
        typed.refresh_from_app(app);
    }
    app.view_stack.push_boxed(overlay);
}

/// Open the live transcript overlay in backtrack-preview mode (#133).
pub(crate) fn open_backtrack_overlay(app: &mut App) {
    let mut overlay = LiveTranscriptOverlay::new();
    overlay.refresh_from_app(app);
    overlay.set_backtrack_preview(0);
    app.view_stack.push(overlay);
    app.status_message =
        Some("Backtrack: \u{2190}/\u{2192} step  Enter rewind  Esc cancel".to_string());
    app.needs_redraw = true;
}

/// Toggle the live transcript overlay on `Ctrl+T`.
pub(crate) fn toggle_live_transcript_overlay(app: &mut App) {
    if app.view_stack.top_kind() == Some(ModalKind::LiveTranscript) {
        app.view_stack.pop();
        app.needs_redraw = true;
        return;
    }
    let mut overlay = LiveTranscriptOverlay::new();
    overlay.refresh_from_app(app);
    app.view_stack.push(overlay);
    app.status_message = Some("Live transcript: tailing (Esc to close)".to_string());
    app.needs_redraw = true;
}

/// Open the `/model` picker pre-filtered to `provider` (#3083).
pub(crate) fn open_model_picker_for_provider(app: &mut App, provider: ApiProvider) {
    if app.view_stack.top_kind() != Some(ModalKind::ModelPicker) {
        app.view_stack
            .push(crate::tui::model_picker::ModelPickerView::new(app));
    }
    for ch in provider.display_name().chars() {
        let _ = app.view_stack.handle_key(crossterm::event::KeyEvent::new(
            KeyCode::Char(ch),
            KeyModifiers::NONE,
        ));
    }
    app.needs_redraw = true;
}

pub(crate) async fn apply_plan_choice(
    app: &mut App,
    config: &Config,
    engine_handle: &EngineHandle,
    choice: PlanChoice,
) -> Result<()> {
    match choice {
        PlanChoice::AcceptAgent => {
            apply_mode_update(app, engine_handle, AppMode::Agent).await;
            app.add_message(HistoryCell::System {
                content: "Plan accepted. Switching to Agent mode and starting implementation."
                    .to_string(),
            });
            let followup = QueuedMessage::new("Proceed with the accepted plan.".to_string(), None);
            if app.is_loading {
                app.queue_message(followup);
                app.status_message =
                    Some("Queued accepted plan execution (agent mode).".to_string());
            } else {
                dispatch_user_message(app, config, engine_handle, followup).await?;
            }
        }
        PlanChoice::AcceptYolo => {
            apply_mode_update(app, engine_handle, AppMode::Yolo).await;
            app.add_message(HistoryCell::System {
                content: "Plan accepted. Switching to YOLO mode and starting implementation."
                    .to_string(),
            });
            let followup = QueuedMessage::new("Proceed with the accepted plan.".to_string(), None);
            if app.is_loading {
                app.queue_message(followup);
                app.status_message =
                    Some("Queued accepted plan execution (YOLO mode).".to_string());
            } else {
                dispatch_user_message(app, config, engine_handle, followup).await?;
            }
        }
        PlanChoice::RevisePlan => {
            let prompt = "Revise the plan: ";
            app.input = prompt.to_string();
            app.cursor_position = prompt.chars().count();
            app.status_message = Some("Revise the plan and press Enter.".to_string());
        }
        PlanChoice::ExitPlan => {
            apply_mode_update(app, engine_handle, AppMode::Agent).await;
            app.add_message(HistoryCell::System {
                content: concat!(
                    "Exited Plan mode. Switched to Agent mode.\n\n",
                    "The plan above is for reference only. ",
                    "Do NOT execute it until the user explicitly asks you to. ",
                    "Wait for the user's next instruction before taking any action.",
                )
                .to_string(),
            });
        }
    }

    Ok(())
}

pub(crate) async fn handle_plan_choice(
    app: &mut App,
    config: &Config,
    engine_handle: &EngineHandle,
    input: &str,
) -> Result<bool> {
    if !app.plan_prompt_pending {
        return Ok(false);
    }

    let choice = parse_plan_choice(input);
    app.plan_prompt_pending = false;

    let Some(choice) = choice else {
        return Ok(false);
    };

    apply_plan_choice(app, config, engine_handle, choice).await?;
    Ok(true)
}

type AppTerminal = ratatui::Terminal<crate::tui::color_compat::ColorCompatBackend<std::io::Stdout>>;

#[allow(clippy::too_many_arguments)]
pub(crate) async fn handle_view_events(
    terminal: &mut AppTerminal,
    app: &mut App,
    config: &mut Config,
    task_manager: &SharedTaskManager,
    engine_handle: &mut EngineHandle,
    web_config_session: &mut Option<WebConfigSession>,
    events: Vec<ViewEvent>,
) -> Result<bool> {
    for event in events {
        match event {
            ViewEvent::CommandPaletteSelected { action } => match action {
                crate::tui::views::CommandPaletteAction::ExecuteCommand { command } => {
                    if super::execute_command_input(
                        terminal,
                        app,
                        engine_handle,
                        task_manager,
                        config,
                        &mut *web_config_session,
                        &command,
                    )
                    .await?
                    {
                        return Ok(true);
                    }
                }
                crate::tui::views::CommandPaletteAction::InsertText { text } => {
                    app.input = text;
                    app.cursor_position = app.input.chars().count();
                    app.status_message = Some(
                        "Inserted into composer. Finish the input or press Enter.".to_string(),
                    );
                }
                crate::tui::views::CommandPaletteAction::OpenTextPager { title, content } => {
                    super::open_text_pager(app, title, content);
                }
            },
            ViewEvent::OpenTextPager { title, content } => {
                super::open_text_pager(app, title, content);
            }
            ViewEvent::CopyToClipboard { text, label } => {
                if text.is_empty() {
                    app.status_message = Some(format!("{label} is empty"));
                } else if app.clipboard.write_text(&text).is_ok() {
                    app.status_message = Some(format!("{label} copied"));
                } else {
                    app.status_message = Some(format!("Copy failed ({label})"));
                }
            }
            ViewEvent::ApprovalDecision {
                tool_id,
                tool_name,
                decision,
                timed_out,
                approval_key,
                approval_grouping_key,
                persistent_ask_rules,
            } => {
                apply_approval_decision(
                    app,
                    engine_handle,
                    config,
                    ApprovalDecisionEvent {
                        tool_id,
                        tool_name,
                        decision,
                        timed_out,
                        approval_key,
                        approval_grouping_key,
                        persistent_ask_rules,
                    },
                )
                .await;

                if timed_out {
                    app.add_message(HistoryCell::System {
                        content: "Approval request timed out - denied".to_string(),
                    });
                }
            }
            ViewEvent::ElevationDecision {
                tool_id,
                tool_name,
                option,
            } => {
                use crate::tui::approval::ElevationOption;
                match option {
                    ElevationOption::Abort => {
                        let _ = engine_handle.deny_tool_call(tool_id).await;
                        app.add_message(HistoryCell::System {
                            content: format!("Sandbox elevation aborted for {tool_name}"),
                        });
                    }
                    ElevationOption::WithNetwork => {
                        app.add_message(HistoryCell::System {
                            content: format!("Retrying {tool_name} with network access enabled"),
                        });
                        let policy = option.to_policy(&app.workspace);
                        let _ = engine_handle.retry_tool_with_policy(tool_id, policy).await;
                    }
                    ElevationOption::WithWriteAccess(_) => {
                        app.add_message(HistoryCell::System {
                            content: format!("Retrying {tool_name} with write access enabled"),
                        });
                        let policy = option.to_policy(&app.workspace);
                        let _ = engine_handle.retry_tool_with_policy(tool_id, policy).await;
                    }
                    ElevationOption::FullAccess => {
                        app.add_message(HistoryCell::System {
                            content: format!("Retrying {tool_name} with full access (no sandbox)"),
                        });
                        let policy = option.to_policy(&app.workspace);
                        let _ = engine_handle.retry_tool_with_policy(tool_id, policy).await;
                    }
                }
            }
            ViewEvent::UserInputSubmitted { tool_id, response } => {
                let _ = engine_handle.submit_user_input(tool_id, response).await;
            }
            ViewEvent::UserInputCancelled { tool_id } => {
                let _ = engine_handle.cancel_user_input(tool_id).await;
                app.add_message(HistoryCell::System {
                    content: "User input cancelled".to_string(),
                });
            }
            ViewEvent::PlanPromptSelected { option } => {
                if app.plan_prompt_pending {
                    app.plan_prompt_pending = false;
                    if let Some(choice) = plan_choice_from_option(option)
                        && let Err(err) =
                            apply_plan_choice(app, config, engine_handle, choice).await
                    {
                        app.status_message = Some(format!("Failed to apply plan selection: {err}"));
                    }
                }
            }
            ViewEvent::PlanPromptDismissed => {
                app.plan_prompt_pending = true;
                app.status_message =
                    Some("Plan prompt closed. Type 1-4 and press Enter to choose.".to_string());
            }
            ViewEvent::SessionSelected { session_id } => {
                let manager = match SessionManager::default_location() {
                    Ok(manager) => manager,
                    Err(err) => {
                        app.status_message =
                            Some(format!("Failed to open sessions directory: {err}"));
                        continue;
                    }
                };

                match manager.load_session(&session_id) {
                    Ok(session) => {
                        let recovered = apply_loaded_session(app, config, &session);
                        sync_runtime_workspace_state(task_manager, app.workspace.clone()).await;
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
                        let _ = engine_handle
                            .send(Op::SetCompaction {
                                config: app.compaction_config(),
                            })
                            .await;
                        if !recovered {
                            app.status_message = Some(format!(
                                "Session loaded (ID: {})",
                                crate::session_manager::truncate_id(&session_id)
                            ));
                        }
                    }
                    Err(err) => {
                        app.status_message = Some(format!(
                            "Failed to load session {}: {err}",
                            crate::session_manager::truncate_id(&session_id)
                        ));
                    }
                }
            }
            ViewEvent::SessionDeleted { session_id, title } => {
                app.status_message = Some(format!(
                    "Deleted session {} ({})",
                    crate::session_manager::truncate_id(&session_id),
                    title
                ));
            }
            ViewEvent::ConfigUpdated {
                key,
                value,
                persist,
            } => {
                let result = crate::commands::set_config_value(app, &key, &value, persist);
                if matches!(
                    key.as_str(),
                    "theme" | "ui_theme" | "background_color" | "background" | "bg"
                ) {
                    app.force_next_full_repaint = true;
                }
                if persist && let Some(msg) = result.message {
                    app.add_message(HistoryCell::System { content: msg });
                }

                if let Some(action) = result.action {
                    match action {
                        super::super::app::AppAction::UpdateCompaction(compaction) => {
                            super::provider_and_model::apply_model_and_compaction_update(
                                engine_handle,
                                compaction,
                                app.mode,
                                app.active_route_limits,
                            )
                            .await;
                        }
                        super::super::app::AppAction::UpdateStreamChunkTimeout(timeout_secs) => {
                            let _ = engine_handle
                                .send(Op::SetStreamChunkTimeout { timeout_secs })
                                .await;
                        }
                        super::super::app::AppAction::UpdateSubagentRuntimeConfig {
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
                        _ => {}
                    }
                }

                if app.view_stack.top_kind() == Some(ModalKind::Config) {
                    app.view_stack.pop();
                    app.view_stack
                        .push(super::super::views::ConfigView::new_for_app(app));
                }
            }
            ViewEvent::StatusItemsUpdated { items, final_save } => {
                app.status_items = items.clone();
                app.needs_redraw = true;
                if final_save {
                    match crate::config_persistence::persist_status_items(&items) {
                        Ok(path) => {
                            app.status_message =
                                Some(format!("Status line saved to {}", path.display()));
                        }
                        Err(err) => {
                            app.add_message(HistoryCell::System {
                                content: format!("Failed to save status line: {err}"),
                            });
                        }
                    }
                }
            }
            ViewEvent::SubAgentsRefresh => {
                app.status_message = Some("Refreshing sub-agents...".to_string());
                let _ = engine_handle.send(Op::ListSubAgents).await;
            }
            ViewEvent::FilePickerSelected { path } => {
                let cursor = app.cursor_position;
                let needs_leading_space = cursor > 0
                    && !app
                        .input
                        .chars()
                        .nth(cursor.saturating_sub(1))
                        .is_some_and(|c| c.is_whitespace());
                let mut insertion = String::new();
                if needs_leading_space {
                    insertion.push(' ');
                }
                insertion.push('@');
                insertion.push_str(&path);
                insertion.push(' ');
                app.insert_str(&insertion);
                app.status_message = Some(format!("Attached @{path}"));
            }
            ViewEvent::ModelPickerApplied {
                model,
                provider,
                effort,
                previous_model,
                previous_effort,
            } => {
                apply_model_picker_choice(
                    app,
                    engine_handle,
                    config,
                    model,
                    provider,
                    effort,
                    previous_model,
                    previous_effort,
                )
                .await;
            }
            ViewEvent::ProviderPickerApplied { provider } => {
                let model_override = provider_picker_model_override(app, provider);
                switch_provider(app, engine_handle, config, provider, model_override).await;
            }
            ViewEvent::ProviderPickerApiKeySubmitted { provider, api_key } => {
                apply_provider_picker_api_key(app, engine_handle, config, provider, api_key).await;
            }
            ViewEvent::ProviderPickerKimiOAuthEnabled { provider } => {
                apply_provider_picker_auth_mode(
                    app,
                    engine_handle,
                    config,
                    provider,
                    "kimi_oauth",
                    "Linked Kimi CLI OAuth",
                )
                .await;
            }
            ViewEvent::ProviderPickerOpenModels { provider } => {
                open_model_picker_for_provider(app, provider);
            }
            ViewEvent::ModeSelected { mode } => {
                let prior_mode = app.mode;
                let msg = crate::commands::switch_mode(app, mode);
                if app.mode != prior_mode {
                    sync_mode_update(engine_handle, app.mode).await;
                }
                app.add_message(HistoryCell::System { content: msg });
            }
            ViewEvent::BacktrackStep { direction } => {
                app.backtrack.step(direction);
                if let Some(idx) = app.backtrack.selected_idx() {
                    update_backtrack_overlay_selection(app, idx);
                }
            }
            ViewEvent::BacktrackConfirm => {
                if let Some(depth) = app.backtrack.confirm() {
                    apply_backtrack(app, depth);
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
            }
            ViewEvent::BacktrackCancel => {
                app.backtrack.reset();
                app.status_message = Some("Backtrack canceled".to_string());
                app.needs_redraw = true;
            }
            ViewEvent::ContextMenuSelected {
                action: super::super::views::ContextMenuAction::ExecuteCommand { command },
            } => {
                if super::execute_command_input(
                    terminal,
                    app,
                    engine_handle,
                    task_manager,
                    config,
                    &mut *web_config_session,
                    &command,
                )
                .await?
                {
                    return Ok(true);
                }
            }
            ViewEvent::ContextMenuSelected { action } => {
                crate::tui::mouse_ui::handle_context_menu_action(app, action)
            }
        }
    }

    Ok(false)
}

fn update_backtrack_overlay_selection(app: &mut App, selected_idx: usize) {
    if app.view_stack.top_kind() != Some(ModalKind::LiveTranscript) {
        return;
    }
    let Some(mut overlay) = app.view_stack.pop() else {
        return;
    };
    if let Some(typed) = overlay.as_any_mut().downcast_mut::<LiveTranscriptOverlay>() {
        typed.set_backtrack_preview(selected_idx);
    }
    app.view_stack.push_boxed(overlay);
    app.needs_redraw = true;
}

pub(crate) fn count_user_history_cells(app: &App) -> usize {
    app.history
        .iter()
        .filter(|cell| matches!(cell, HistoryCell::User { .. }))
        .count()
}

fn find_user_cell_index_from_tail(app: &App, depth: usize) -> Option<usize> {
    let mut count = 0usize;
    for (idx, cell) in app.history.iter().enumerate().rev() {
        if matches!(cell, HistoryCell::User { .. }) {
            if count == depth {
                return Some(idx);
            }
            count += 1;
        }
    }
    None
}

fn apply_backtrack(app: &mut App, depth: usize) {
    let Some(history_idx) = find_user_cell_index_from_tail(app, depth) else {
        app.status_message = Some("Backtrack target no longer present".to_string());
        return;
    };

    let user_text = match app.history.get(history_idx) {
        Some(HistoryCell::User { content }) => content.clone(),
        _ => String::new(),
    };

    app.truncate_history_to(history_idx);

    let mut user_seen = 0usize;
    let mut cut = None;
    for (idx, msg) in app.api_messages.iter().enumerate().rev() {
        if msg.role == "user" {
            if user_seen == depth {
                cut = Some(idx);
                break;
            }
            user_seen += 1;
        }
    }
    if let Some(idx) = cut {
        app.api_messages.truncate(idx);
    }

    app.input = user_text;
    app.cursor_position = app.input.chars().count();

    if app.view_stack.top_kind() == Some(ModalKind::LiveTranscript) {
        app.view_stack.pop();
    }
    app.status_message =
        Some("Rewound to previous user message — edit and Enter to resend".to_string());
    app.scroll_to_bottom();
    app.mark_history_updated();
    app.needs_redraw = true;
}
