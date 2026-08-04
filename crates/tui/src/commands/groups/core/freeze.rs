//! `/freeze` and `/unfreeze` commands for spec freeze (#557).

use crate::commands::traits::{CommandInfo, RegisterCommand};
use crate::localization::MessageId;
use crate::tui::app::{App, AppAction};

use super::CommandResult;

pub(in crate::commands) const FREEZE_INFO: CommandInfo = CommandInfo {
    name: "freeze",
    aliases: &[],
    usage: "/freeze [spec text]",
    description_id: MessageId::CmdFreezeDescription,
};

pub(in crate::commands) struct FreezeCmd;

impl RegisterCommand for FreezeCmd {
    fn info() -> &'static CommandInfo {
        &FREEZE_INFO
    }

    fn execute(app: &mut App, arg: Option<&str>) -> CommandResult {
        let spec = match arg {
            Some(text) if !text.trim().is_empty() => Some(text.trim().to_string()),
            Some(_) | None => None,
        };

        app.spec_frozen = true;
        app.frozen_spec = spec.clone();

        let message = match &spec {
            None => "Spec frozen. Agent will be constrained to the current plan.".to_string(),
            Some(s) => format!(
                "Spec frozen with {} chars. Agent will be constrained to this spec.",
                s.len()
            ),
        };

        CommandResult::with_message_and_action(message, AppAction::SpecFrozen)
    }
}

pub(in crate::commands) const UNFREEZE_INFO: CommandInfo = CommandInfo {
    name: "unfreeze",
    aliases: &[],
    usage: "/unfreeze",
    description_id: MessageId::CmdUnfreezeDescription,
};

pub(in crate::commands) struct UnfreezeCmd;

impl RegisterCommand for UnfreezeCmd {
    fn info() -> &'static CommandInfo {
        &UNFREEZE_INFO
    }

    fn execute(app: &mut App, _arg: Option<&str>) -> CommandResult {
        app.spec_frozen = false;
        app.frozen_spec = None;

        CommandResult::with_message_and_action(
            "Spec unfrozen. Agent is no longer constrained.".to_string(),
            AppAction::SpecUnfrozen,
        )
    }
}
