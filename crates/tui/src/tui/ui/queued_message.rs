//! queued message 子系统（从 ui 上帝文件切片）
use super::*;

pub(crate) fn build_queued_message(app: &mut App, input: String) -> QueuedMessage {
    let skill_instruction = app.active_skill.take();
    QueuedMessage::new(input, skill_instruction)
}

const INITIAL_PROMPT_DEFERRED_STATUS: &str = "Initial prompt ready; complete setup to send it";

pub(crate) async fn submit_initial_input_if_ready(
    app: &mut App,
    config: &Config,
    engine_handle: &EngineHandle,
) -> Result<()> {
    if !app.auto_submit_initial_input {
        return Ok(());
    }

    if app.onboarding != OnboardingState::None {
        if app.status_message.is_none() && !app.input.trim().is_empty() {
            app.status_message = Some(INITIAL_PROMPT_DEFERRED_STATUS.to_string());
        }
        return Ok(());
    }

    app.auto_submit_initial_input = false;
    if let Some(input) = app.submit_input() {
        if app.status_message.as_deref() == Some(INITIAL_PROMPT_DEFERRED_STATUS) {
            app.status_message = None;
        }
        let queued = build_queued_message(app, input);
        dispatch_user_message(app, config, engine_handle, queued).await?;
    }
    Ok(())
}

pub(crate) fn queue_current_draft_for_next_turn(app: &mut App) -> bool {
    let Some(input) = app.submit_input() else {
        return false;
    };
    let queued = if let Some(mut draft) = app.queued_draft.take() {
        draft.display = input;
        draft
    } else {
        build_queued_message(app, input)
    };
    app.queue_message(queued);
    app.status_message = Some(format!(
        "{} queued follow-up(s) — ↑ edit last, /queue send <n>",
        app.queued_message_count()
    ));
    true
}

fn take_ctrl_s_queued_message(app: &mut App) -> Option<(QueuedMessage, Option<usize>)> {
    if let Some(mut draft) = app.queued_draft.take() {
        if let Some(input) = app.submit_input() {
            draft.display = input;
        }
        return Some((draft, None));
    }
    if app.input.is_empty() {
        return app
            .remove_queued_message(0)
            .map(|message| (message, Some(0)));
    }
    None
}

pub(crate) async fn send_ctrl_s_queued_message_now(
    app: &mut App,
    config: &Config,
    engine_handle: &EngineHandle,
) -> Result<bool> {
    let Some((message, restore_index)) = take_ctrl_s_queued_message(app) else {
        return Ok(false);
    };
    send_taken_queued_message_now(app, config, engine_handle, message, restore_index).await?;
    Ok(true)
}

pub(crate) async fn send_queued_message_at_index_now(
    app: &mut App,
    config: &Config,
    engine_handle: &EngineHandle,
    index: usize,
) -> Result<bool> {
    let Some(message) = app.remove_queued_message(index) else {
        app.status_message = Some("Queued message not found".to_string());
        return Ok(true);
    };
    send_taken_queued_message_now(app, config, engine_handle, message, Some(index)).await?;
    Ok(true)
}

async fn send_taken_queued_message_now(
    app: &mut App,
    config: &Config,
    engine_handle: &EngineHandle,
    message: QueuedMessage,
    restore_index: Option<usize>,
) -> Result<()> {
    if app.offline_mode {
        restore_queued_message(app, restore_index, message);
        app.status_message = Some(format!(
            "Offline: {} queued follow-up(s) — /queue send <n>, /queue clear",
            app.queued_message_count()
        ));
        return Ok(());
    }

    let display = message.display.clone();
    if app.is_loading {
        if let Err(err) = steer_user_message(app, engine_handle, message.clone()).await {
            restore_queued_message(app, restore_index, message);
            app.status_message = Some(format!(
                "Steer failed ({err}); {} queued follow-up(s) — /queue send <n>, /queue clear",
                app.queued_message_count()
            ));
        } else {
            app.push_status_toast(
                "Sent queued follow-up into current turn",
                StatusToastLevel::Info,
                Some(1_500),
            );
        }
    } else if let Err(err) =
        dispatch_user_message(app, config, engine_handle, message.clone()).await
    {
        restore_queued_message(app, restore_index, message);
        app.status_message = Some(format!(
            "Dispatch failed ({err}); kept {} queued follow-up(s)",
            app.queued_message_count()
        ));
    } else {
        app.status_message = Some(format!("Sent queued follow-up: {display}"));
    }
    Ok(())
}

fn restore_queued_message(app: &mut App, index: Option<usize>, message: QueuedMessage) {
    if let Some(index) = index
        && index <= app.queued_messages.len()
    {
        app.queued_messages.insert(index, message);
    } else {
        app.queue_message(message);
    }
}

pub(crate) fn queued_message_content_for_app(
    app: &App,
    message: &QueuedMessage,
    cwd: Option<PathBuf>,
) -> String {
    // Pass the process CWD explicitly so the resolver's two-pass logic can
    // honor the user's launch directory when it differs from `--workspace`
    // (issue #101 — file mentions silently routing to the wrong root).
    let user_request = crate::tui::file_mention::user_request_with_file_mentions(
        &message.display,
        &app.workspace,
        cwd,
    );
    if let Some(skill_instruction) = message.skill_instruction.as_ref() {
        format!("{skill_instruction}\n\n---\n\nUser request: {user_request}")
    } else {
        user_request
    }
}
