//! `/exit_plan` command — leave Plan mode and return to the default mode.

use crate::commands::traits::{CommandInfo, RegisterCommand};
use crate::localization::MessageId;
use crate::tui::app::{App, AppAction, AppMode};

use super::CommandResult;

pub(in crate::commands) const COMMAND_INFO: CommandInfo = CommandInfo {
    name: "exit_plan",
    aliases: &["leave_plan", "tuichu_plan"],
    usage: "/exit_plan",
    description_id: MessageId::CmdExitPlanDescription,
};

pub(in crate::commands) struct ExitPlanCmd;

impl RegisterCommand for ExitPlanCmd {
    fn info() -> &'static CommandInfo {
        &COMMAND_INFO
    }

    fn execute(app: &mut App, _arg: Option<&str>) -> CommandResult {
        if app.mode != AppMode::Plan {
            return CommandResult::message("当前不在 Plan 模式，无需退出。".to_string());
        }
        CommandResult::with_message_and_action(
            "已退出 Plan 模式，回到默认模式。".to_string(),
            AppAction::ModeChanged(AppMode::Agent),
        )
    }
}
