//! `/auto` command.

use crate::commands::groups::config::config::switch_mode;
use crate::commands::traits::{CommandInfo, RegisterCommand};
use crate::localization::MessageId;
use crate::tui::app::{App, AppAction, AppMode};

use super::CommandResult;

pub(in crate::commands) const COMMAND_INFO: CommandInfo = CommandInfo {
    name: "auto",
    aliases: &[],
    usage: "/auto",
    description_id: MessageId::CmdAutoModeDescription,
};

pub(in crate::commands) struct AutoCmd;

impl RegisterCommand for AutoCmd {
    fn info() -> &'static CommandInfo {
        &COMMAND_INFO
    }

    fn execute(app: &mut App, _arg: Option<&str>) -> CommandResult {
        let message = switch_mode(app, AppMode::Agent);
        CommandResult::with_message_and_action(message, AppAction::ModeChanged(AppMode::Agent))
    }
}
