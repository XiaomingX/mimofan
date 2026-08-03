//! foreground shell 子系统（从 ui 上帝文件切片）
use super::*;

pub(crate) fn request_foreground_shell_background(app: &mut App) {
    if !app.is_loading {
        app.status_message = Some("No foreground shell command to background".to_string());
        return;
    }
    if !active_foreground_shell_running(app) {
        // #3032 AC3: name the reason backgrounding is unavailable —
        // interactive execs and non-shell blocking tools are visibly running
        // but cannot be detached, and a generic shrug reads like a bug.
        let reason = if terminal_pause_has_live_owner(app) {
            "the running command is interactive"
        } else if app
            .active_cell
            .as_ref()
            .is_some_and(|active| !active.is_empty())
        {
            "the running tool is not a foreground shell command"
        } else {
            "no foreground shell command is running"
        };
        app.status_message = Some(format!(
            "Cannot background: {reason}. Press Ctrl+C to cancel the turn, or wait for completion."
        ));
        return;
    }

    let Some(shell_manager) = app.runtime_services.shell_manager.clone() else {
        app.status_message = Some("Shell manager is not attached".to_string());
        return;
    };

    match shell_manager.lock() {
        Ok(mut manager) => {
            manager.request_foreground_background();
            app.status_message = Some("Backgrounding current shell command...".to_string());
        }
        Err(_) => {
            app.status_message = Some("Shell manager lock is poisoned".to_string());
        }
    }
}

pub(crate) fn prefill_jobs_cancel_all_if_tasks_sidebar(app: &mut App) -> bool {
    if !app.view_stack.is_empty()
        || app.sidebar_focus != SidebarFocus::Tasks
        || !app
            .task_panel
            .iter()
            .any(|task| task.id.starts_with("shell_") && task.status == "running")
    {
        return false;
    }

    app.input = "/jobs cancel-all".to_string();
    app.cursor_position = app.input.len();
    app.status_message = Some("Press Enter to cancel all running commands".to_string());
    true
}

pub(crate) fn active_foreground_shell_running(app: &App) -> bool {
    app.active_cell.as_ref().is_some_and(|active| {
        active.entries().iter().any(|cell| {
            matches!(
                cell,
                HistoryCell::Tool(ToolCell::Exec(exec))
                    if exec.status == ToolStatus::Running && exec.interaction.is_none()
            )
        })
    })
}

pub(crate) fn terminal_pause_has_live_owner(app: &App) -> bool {
    app.active_cell.as_ref().is_some_and(|active| {
        active.entries().iter().any(|cell| {
            matches!(
                cell,
                HistoryCell::Tool(ToolCell::Exec(exec)) if exec.status == ToolStatus::Running
            )
        })
    })
}
