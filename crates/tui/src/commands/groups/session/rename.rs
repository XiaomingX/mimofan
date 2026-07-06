//! `/rename` command — set a custom title for the current session.

use crate::commands::traits::{CommandInfo, RegisterCommand};
use crate::localization::MessageId;
use crate::session_manager::{SessionManager, update_session};
use crate::tui::app::App;

use super::CommandResult;

const MAX_TITLE_LEN: usize = 100;

pub(in crate::commands) const COMMAND_INFO: CommandInfo = CommandInfo {
    name: "rename",
    aliases: &["gaiming", "chongmingming"],
    usage: "/rename <new title>",
    description_id: MessageId::CmdRenameDescription,
};

pub(in crate::commands) struct RenameCmd;

impl RegisterCommand for RenameCmd {
    fn info() -> &'static CommandInfo {
        &COMMAND_INFO
    }

    fn execute(app: &mut App, arg: Option<&str>) -> CommandResult {
        rename(app, arg)
    }
}

/// Rename the current session to the given title.
///
/// Usage: `/rename <new title>`
///
/// The new title is persisted immediately to `~/.mimofanfan/sessions/<id>.json`
/// so the updated name is visible the next time the session picker is opened.
pub fn rename(app: &mut App, arg: Option<&str>) -> CommandResult {
    let new_title = match arg.map(str::trim).filter(|s| !s.is_empty()) {
        Some(t) => t,
        None => return CommandResult::error("Usage: /rename <new title>"),
    };

    if new_title.chars().count() > MAX_TITLE_LEN {
        return CommandResult::error(format!("Title too long (max {MAX_TITLE_LEN} characters)"));
    }

    let session_id = match &app.current_session_id {
        Some(id) => id.clone(),
        None => {
            return CommandResult::error(
                "No active session. Send a message first to start a session.",
            );
        }
    };

    let manager = match SessionManager::default_location() {
        Ok(m) => m,
        Err(e) => return CommandResult::error(format!("Could not open sessions directory: {e}")),
    };

    rename_with_manager(new_title, &session_id, &manager, app)
}

fn rename_with_manager(
    new_title: &str,
    session_id: &str,
    manager: &SessionManager,
    app: &App,
) -> CommandResult {
    let mut session = match manager.load_session(session_id) {
        Ok(s) => s,
        Err(e) => return CommandResult::error(format!("Could not load session: {e}")),
    };

    // Sync with current App state to avoid overwriting unsaved messages.
    session = update_session(
        session,
        &app.api_messages,
        u64::from(app.session.total_tokens),
        app.system_prompt.as_ref(),
    );
    app.sync_cost_to_metadata(&mut session.metadata);
    session.metadata.title = new_title.to_string();

    match manager.save_session(&session) {
        Ok(_) => CommandResult::message(format!("Session renamed to \"{new_title}\"")),
        Err(e) => CommandResult::error(format!("Could not save session: {e}")),
    }
}

#[cfg(test)]
mod tests {}
