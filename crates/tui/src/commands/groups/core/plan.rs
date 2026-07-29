//! `/plan` command.

use crate::commands::groups::config::config::switch_mode;
use crate::commands::traits::{CommandInfo, RegisterCommand};
use crate::localization::MessageId;
use crate::tui::app::{App, AppAction, AppMode};

use super::CommandResult;

pub(in crate::commands) const COMMAND_INFO: CommandInfo = CommandInfo {
    name: "plan",
    aliases: &[],
    usage: "/plan",
    description_id: MessageId::CmdPlanModeDescription,
};

pub(in crate::commands) struct PlanCmd;

impl RegisterCommand for PlanCmd {
    fn info() -> &'static CommandInfo {
        &COMMAND_INFO
    }

    fn execute(app: &mut App, _arg: Option<&str>) -> CommandResult {
        let message = switch_mode(app, AppMode::Plan);
        CommandResult::with_message_and_action(message, AppAction::ModeChanged(AppMode::Plan))
    }
}
