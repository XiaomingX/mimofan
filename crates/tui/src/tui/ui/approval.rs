//! approval 子系统（从 ui 上帝文件切片）
use super::*;

pub(crate) fn push_approval_request_view(
    app: &mut App,
    id: &str,
    tool_name: &str,
    description: &str,
    tool_input: &serde_json::Value,
    approval_key: &str,
    intent_summary: Option<&str>,
) {
    if tool_name == "apply_patch" {
        maybe_add_patch_preview(app, tool_input);
    }

    let request = ApprovalRequest::new_with_intent(
        id,
        tool_name,
        description,
        tool_input,
        approval_key,
        intent_summary,
        &app.workspace,
    );
    app.view_stack
        .push(ApprovalView::new_for_locale(request, app.ui_locale));
}

pub(crate) struct ApprovalDecisionEvent {
    pub(crate) tool_id: String,
    pub(crate) tool_name: String,
    pub(crate) decision: ReviewDecision,
    pub(crate) timed_out: bool,
    pub(crate) approval_key: String,
    pub(crate) approval_grouping_key: String,
    pub(crate) persistent_ask_rules: Vec<mimofan_config::ToolAskRule>,
}

pub(crate) async fn apply_approval_decision(
    app: &mut App,
    engine_handle: &mut EngineHandle,
    config: &mut Config,
    event: ApprovalDecisionEvent,
) {
    if event.decision == ReviewDecision::ApprovedForSession {
        // Store the tool name (backward compat) and the lossy grouping key so
        // later flag variants of the same command family are also auto-approved
        // (v0.8.37).
        app.approval_session_approved
            .insert(event.tool_name.clone());
        app.approval_session_approved
            .insert(event.approval_grouping_key.clone());
    }

    if matches!(
        event.decision,
        ReviewDecision::Approved | ReviewDecision::ApprovedForSession
    ) && !event.persistent_ask_rules.is_empty()
        && !event.timed_out
    {
        persist_ask_rules_from_approval(app, config, &event.persistent_ask_rules);
    }

    match event.decision {
        ReviewDecision::Approved | ReviewDecision::ApprovedForSession => {
            let _ = engine_handle.approve_tool_call(event.tool_id).await;
        }
        ReviewDecision::Denied => {
            // Cache the denial so the model retry-loop doesn't re-prompt for
            // the exact same approval_key (#360). Only the key (per-call
            // unique) is stored — NOT the tool_name, which would block all
            // future invocations of the same tool type (#1377).
            if !event.timed_out {
                app.approval_session_denied.insert(event.approval_key);
            }
            let _ = engine_handle.deny_tool_call(event.tool_id).await;
        }
        ReviewDecision::Abort => {
            engine_handle.cancel();
            mark_active_turn_cancelled_locally(app);
            app.status_message = Some("Request cancelled".to_string());
        }
    }
}

fn persist_ask_rules_from_approval(
    app: &mut App,
    config: &mut Config,
    rules: &[mimofan_config::ToolAskRule],
) {
    match mimofan_config::ConfigStore::load(app.config_path.clone()).and_then(|mut store| {
        let added = store.append_ask_rules(rules)?;
        let permissions_path = store.permissions_path();
        config.exec_policy_engine = store.exec_policy_engine();
        Ok((added, permissions_path))
    }) {
        Ok((added, path)) if added > 0 => {
            app.status_message = Some(format!(
                "Saved {added} ask permission rule(s) to {}",
                path.display()
            ));
        }
        Ok((_added, path)) => {
            app.status_message = Some(format!(
                "Ask permission rule already saved in {}",
                path.display()
            ));
        }
        Err(err) => {
            app.status_message = Some(format!("Failed to save ask permission rule: {err:#}"));
        }
    }
}

pub(crate) fn mark_active_turn_cancelled_locally(app: &mut App) {
    // #2739: every local cancel surface (Esc, Ctrl+C, approval abort, paused
    // command abort) must snapshot before it clears turn state. Otherwise
    // --continue reloads the previous save and the interrupted turn vanishes.
    app.streaming_state.reset();
    app.finalize_active_cell_as_interrupted();

    // Extract the partial assistant text BEFORE finalize_streaming_assistant_as_interrupted
    // calls .take() on streaming_message_index, so we can push it to api_messages
    // for persistence.
    let partial_assistant_text = app
        .streaming_message_index
        .and_then(|i| app.history.get(i))
        .and_then(|cell| match cell {
            HistoryCell::Assistant { content, .. } if !content.is_empty() => Some(content.clone()),
            _ => None,
        });

    app.finalize_streaming_assistant_as_interrupted();

    // Push the partial assistant text into api_messages so persist_recovery_snapshot
    // saves it. Without this, the streamed-but-interrupted content would be lost
    // because push_assistant_message only fires on MessageComplete (never on cancel).
    if let Some(text) = partial_assistant_text {
        app.api_messages.push(Message {
            role: "assistant".to_string(),
            content: vec![ContentBlock::Text {
                text,
                cache_control: None,
            }],
        });
    }

    persist_recovery_snapshot(app);
    app.is_loading = false;
    app.dispatch_started_at = None;
    app.turn_started_at = None;
    app.turn_last_activity_at = None;
    app.runtime_turn_id = None;
    app.runtime_turn_status = None;
    app.suppress_stream_events_until_turn_complete = true;
    crate::retry_status::clear();
    crate::tui::notifications::clear_taskbar_progress();
    crate::tui::notifications::stop_title_animation_quietly();
}

pub(crate) fn suppress_engine_event_after_local_cancel(event: &EngineEvent) -> bool {
    matches!(
        event,
        EngineEvent::MessageStarted { .. }
            | EngineEvent::MessageDelta { .. }
            | EngineEvent::MessageComplete { .. }
            | EngineEvent::ThinkingStarted { .. }
            | EngineEvent::ThinkingDelta { .. }
            | EngineEvent::ThinkingComplete { .. }
            | EngineEvent::ToolCallStarted { .. }
            | EngineEvent::ToolCallComplete { .. }
            | EngineEvent::ApprovalRequired { .. }
            | EngineEvent::UserInputRequired { .. }
            // Leaving Plan mode is a privilege escalation (read-only ->
            // writable). If the user cancelled the turn, honour the cancel
            // rather than dropping them into Agent mode.
            | EngineEvent::PlanModeApproved { .. }
            | EngineEvent::ElevationRequired { .. }
            | EngineEvent::SessionUpdated { .. }
    )
}

pub(crate) fn ignore_stale_stream_event_while_idle(event: &EngineEvent) -> bool {
    matches!(
        event,
        EngineEvent::MessageStarted { .. }
            | EngineEvent::MessageDelta { .. }
            | EngineEvent::MessageComplete { .. }
            | EngineEvent::ThinkingStarted { .. }
            | EngineEvent::ThinkingDelta { .. }
            | EngineEvent::ThinkingComplete { .. }
            | EngineEvent::ToolCallStarted { .. }
            | EngineEvent::ToolCallComplete { .. }
            | EngineEvent::ApprovalRequired { .. }
            | EngineEvent::UserInputRequired { .. }
            | EngineEvent::ElevationRequired { .. }
    )
}
