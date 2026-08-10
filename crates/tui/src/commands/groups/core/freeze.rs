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

        let Some(spec) = spec else {
            // No plan supplied. The engine (`prompts/mod.rs`) only injects a
            // constraint when `frozen_spec` is non-empty, so freezing with an
            // empty spec would *look* successful while binding nothing. Say so
            // plainly and leave the existing freeze state untouched.
            return CommandResult::message(
                "No spec provided, so nothing was frozen. To constrain the agent, run `/freeze <plan text>`.".to_string(),
            );
        };

        app.spec_frozen = true;
        app.frozen_spec = Some(spec.clone());

        let message = format!(
            "Spec frozen with {} chars. Agent will be constrained to this spec.",
            spec.len()
        );

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
