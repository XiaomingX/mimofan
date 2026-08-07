//! User message dispatch, steering, queuing, and submission logic.

use std::time::Instant;

use anyhow::Result;

use crate::config::Config;
use crate::core::engine::EngineHandle;
use crate::core::ops::Op;
use crate::models::{ContentBlock, Message};
use crate::session_manager::SessionManager;
use crate::tui::auto_router;
use crate::tui::persistence_actor::{self, PersistRequest};

use super::super::app::{App, QueuedMessage, ReasoningEffort, StatusToastLevel, SubmitDisposition};
use super::super::history::HistoryCell;
use super::context_usage::{maybe_warn_context_pressure, should_auto_compact_before_send};
use super::engine_config_prompt::build_app_system_prompt;
use super::paused_command::prepare_paused_command_message;
use super::queued_message::queued_message_content_for_app;
use super::session_warmup::build_session_snapshot;

/// Dispatch a user message to the engine.
pub(crate) async fn dispatch_user_message(
    app: &mut App,
    config: &Config,
    engine_handle: &EngineHandle,
    mut message: QueuedMessage,
) -> Result<()> {
    // #1364: run mutable `message_submit` hooks before dispatch. Hooks see the
    // user's display text and may replace or block it before file mentions,
    // skill wrapping, history, and model input are resolved.
    // Fast-path skip when no hooks configured.
    if app
        .hooks
        .has_hooks_for_event(crate::hooks::HookEvent::MessageSubmit)
    {
        let context = app.base_hook_context().with_message(&message.display);
        let outcome = app
            .hooks
            .execute_message_submit_transform(&context, &message.display);
        if let Some(warning) = outcome.warning() {
            app.status_message = Some(warning.to_string());
        }
        match outcome {
            crate::hooks::MessageSubmitOutcome::Unchanged { .. } => {}
            crate::hooks::MessageSubmitOutcome::Replaced { text, .. } => {
                message.display = text;
            }
            crate::hooks::MessageSubmitOutcome::Blocked { reason } => {
                app.status_message = Some(reason);
                app.is_loading = false;
                app.dispatch_started_at = None;
                app.runtime_turn_status = None;
                return Ok(());
            }
        }
    }

    let paused_note = prepare_paused_command_message(app, engine_handle, &message.display);

    // Set immediately to prevent double-dispatch before TurnStarted event arrives.
    let dispatch_started_at = Instant::now();
    app.is_loading = true;
    app.dispatch_started_at = Some(dispatch_started_at);
    app.runtime_turn_status = None;
    app.last_send_at = Some(dispatch_started_at);
    app.last_submitted_prompt = Some(message.display.clone());
    // Clear the previous turn's receipt and evidence.
    app.clear_receipt();
    app.tool_evidence.clear();

    let cwd = std::env::current_dir().ok();
    let references = crate::tui::file_mention::context_references_from_input(
        &message.display,
        &app.workspace,
        cwd.clone(),
    );
    let mut content = queued_message_content_for_app(app, &message, cwd);
    if let Some(note) = paused_note.as_deref() {
        content.push_str(note);
    }
    let auto_selection = if auto_router::should_resolve_auto_model_selection(app) {
        match auto_router::resolve_auto_model_selection(app, config, &message, &content).await {
            Ok(selection) => Some(selection),
            Err(err) => {
                app.is_loading = false;
                app.dispatch_started_at = None;
                app.last_send_at = None;
                app.status_message = Some(format!("Auto model route unavailable: {err}"));
                return Err(err);
            }
        }
    } else {
        None
    };
    let effective_provider = auto_selection
        .as_ref()
        .map(|selection| selection.provider)
        .unwrap_or(app.api_provider);
    let message_index = app.api_messages.len();
    app.system_prompt = Some(build_app_system_prompt(app, config));
    app.add_message(HistoryCell::User {
        content: message.display.clone(),
    });
    let history_cell = app.history.len().saturating_sub(1);
    app.record_context_references(history_cell, message_index, references);
    app.scroll_to_bottom();
    app.api_messages.push(Message {
        role: "user".to_string(),
        content: vec![ContentBlock::Text {
            text: content.clone(),
            cache_control: None,
        }],
    });
    maybe_warn_context_pressure(app);
    if should_auto_compact_before_send(app) {
        app.status_message =
            Some("Context threshold reached; compacting before send...".to_string());
        let _ = engine_handle.send(Op::CompactContext).await;
    }
    app.session.last_prompt_tokens = None;
    app.session.last_completion_tokens = None;
    app.session.last_output_throughput = None;
    app.session.last_prompt_cache_hit_tokens = None;
    app.session.last_prompt_cache_miss_tokens = None;
    app.session.last_reasoning_replay_tokens = None;
    // Persist immediately so abrupt termination can recover this in-flight turn.
    // Offloaded to the persistence actor.
    if let Ok(manager) = SessionManager::default_location() {
        let session = build_session_snapshot(app, &manager);
        persistence_actor::persist(PersistRequest::Checkpoint(session.clone()));
        persistence_actor::persist_plan_state(
            session.metadata.id.clone(),
            app.current_plan_and_todo(),
        );
    }

    let effective_model = if app.auto_model {
        auto_selection
            .as_ref()
            .map(|selection| selection.model.clone())
            .unwrap_or_else(|| {
                crate::model_routing::auto_model_heuristic(&message.display, &app.model)
            })
    } else {
        app.model.clone()
    };

    let auto_controls_reasoning = app.auto_model || app.reasoning_effort == ReasoningEffort::Auto;
    let effective_reasoning_effort = if auto_controls_reasoning {
        let effort = auto_selection
            .as_ref()
            .and_then(|selection| selection.reasoning_effort)
            .unwrap_or_else(|| {
                auto_router::normalize_auto_routed_effort(crate::auto_reasoning::select(
                    false,
                    &message.display,
                ))
            });
        app.last_effective_reasoning_effort = Some(effort);
        effort
            .api_value_for_provider(effective_provider)
            .map(str::to_string)
    } else {
        app.last_effective_reasoning_effort = None;
        app.reasoning_effort
            .api_value_for_provider(effective_provider)
            .map(str::to_string)
    };

    if let Some(selection) = auto_selection.as_ref() {
        if app.auto_model {
            app.last_effective_model = Some(effective_model.clone());
            let mut status = format!(
                "Auto model selected: {} / {effective_model} via {}",
                selection.provider.display_name(),
                selection.source.label()
            );
            if let Some(effort) = app.last_effective_reasoning_effort {
                status.push_str(&format!(
                    "; thinking auto: {}",
                    effort.display_label_for_provider(effective_provider)
                ));
            }
            app.status_message = Some(status);
        }
    } else {
        app.last_effective_model = None;
    }

    if let Err(err) = engine_handle
        .send(Op::SendMessage {
            content,
            mode: app.mode,
            provider: Some(effective_provider),
            model: effective_model,
            goal_objective: app.hunt.quarry.clone(),
            goal_token_budget: app.hunt.token_budget,
            goal_status: app.hunt.verdict.goal_status(),
            reasoning_effort: effective_reasoning_effort,
            reasoning_effort_auto: auto_controls_reasoning,
            response_format: None,
            auto_model: app.auto_model,
            allow_shell: app.allow_shell,
            trust_mode: app.trust_mode,
            auto_approve: app.mode == crate::tui::app::AppMode::Yolo,
            approval_mode: app.approval_mode,
            translation_enabled: app.translation_enabled,
            show_thinking: app.show_thinking,
            allowed_tools: app.active_allowed_tools.clone(),
            dynamic_tools: Vec::new(),
            hook_executor: app.runtime_services.hook_executor.clone(),
            verbosity: app.verbosity.clone(),
            provenance: crate::core::ops::UserInputProvenance::ExternalUser,
        })
        .await
    {
        app.is_loading = false;
        app.dispatch_started_at = None;
        app.last_send_at = None;
        return Err(err);
    }

    Ok(())
}

/// Steer a message into the current running turn.
pub(crate) async fn steer_user_message(
    app: &mut App,
    engine_handle: &EngineHandle,
    message: QueuedMessage,
) -> Result<()> {
    let paused_note = prepare_paused_command_message(app, engine_handle, &message.display);
    let cwd = std::env::current_dir().ok();
    let references = crate::tui::file_mention::context_references_from_input(
        &message.display,
        &app.workspace,
        cwd.clone(),
    );
    let mut content = queued_message_content_for_app(app, &message, cwd);
    if let Some(note) = paused_note.as_deref() {
        content.push_str(note);
    }
    let message_index = app.api_messages.len();

    engine_handle.steer(content.clone()).await?;
    app.last_submitted_prompt = Some(message.display.clone());

    // Flush any streaming thinking/tool content into history before
    // inserting the steer message, so the steer appears after (below)
    // the content that chronologically preceded it.
    app.flush_active_cell();

    // Mirror steer input in local transcript/session state.
    app.add_message(HistoryCell::User {
        content: format!("+ {}", message.display),
    });
    let history_cell = app.history.len().saturating_sub(1);
    app.record_context_references(history_cell, message_index, references);
    app.api_messages.push(Message {
        role: "user".to_string(),
        content: vec![ContentBlock::Text {
            text: content.clone(),
            cache_control: None,
        }],
    });

    app.status_message = Some("Steering current turn...".to_string());
    Ok(())
}

/// Park a draft on the queued-messages bucket for dispatch after TurnComplete.
/// Unlike a steer, the message is NOT forwarded immediately — it waits for
/// the current turn to finish, then dispatches as a normal user message.
pub(crate) async fn queue_follow_up(app: &mut App, message: QueuedMessage) -> Result<()> {
    let display = message.display.clone();
    app.queue_message(message);
    app.status_message = Some(format!(
        "Queued: {} ({} total) — ↑ to edit",
        display,
        app.queued_message_count()
    ));
    Ok(())
}

/// Decide whether to dispatch immediately, queue, steer, or queue-follow-up.
pub(crate) async fn submit_or_steer_message(
    app: &mut App,
    config: &Config,
    engine_handle: &EngineHandle,
    message: QueuedMessage,
) -> Result<()> {
    match app.decide_submit_disposition() {
        SubmitDisposition::Immediate => {
            dispatch_user_message(app, config, engine_handle, message).await
        }
        SubmitDisposition::Queue => {
            let count = app.queued_message_count().saturating_add(1);
            app.queue_message(message);
            if app.offline_mode {
                app.status_message = Some(format!(
                    "Offline: {count} queued follow-up(s) — ↑ edit last, /queue send <n>"
                ));
            } else {
                app.status_message = Some(format!(
                    "{count} queued follow-up(s) — ↑ edit last, /queue send <n>"
                ));
            }
            Ok(())
        }
        // Steer: reached via Enter when busy-but-waiting (v0.8.44), or
        // via Ctrl+Enter override in any busy state.
        SubmitDisposition::Steer => {
            if let Err(err) = steer_user_message(app, engine_handle, message.clone()).await {
                app.queue_message(message);
                app.status_message = Some(format!(
                    "Steer failed ({err}); {} queued follow-up(s) — /queue send <n>",
                    app.queued_message_count()
                ));
            } else {
                app.push_status_toast(
                    "Steering into current turn",
                    StatusToastLevel::Info,
                    Some(1_500),
                );
            }
            Ok(())
        }
        SubmitDisposition::QueueFollowUp => queue_follow_up(app, message).await,
    }
}

/// Drain `app.pending_steers` into a single `QueuedMessage` ready for
/// `dispatch_user_message`. Returns `None` if the queue was empty (caller
/// then falls back to `app.queued_messages`). Skill instruction is taken
/// from the first message that supplies one — multiple steers shouldn't
/// double-up the system framing.
pub(crate) fn merge_pending_steers(app: &mut App) -> Option<QueuedMessage> {
    let drained = app.drain_pending_steers();
    if drained.is_empty() {
        return None;
    }
    if drained.len() == 1 {
        return drained.into_iter().next();
    }
    let mut skill_instruction: Option<String> = None;
    let mut bodies: Vec<String> = Vec::with_capacity(drained.len());
    for msg in drained {
        if skill_instruction.is_none() {
            skill_instruction = msg.skill_instruction;
        }
        bodies.push(msg.display);
    }
    Some(QueuedMessage::new(bodies.join("\n\n"), skill_instruction))
}
