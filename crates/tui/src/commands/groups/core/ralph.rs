//! `/ralph` command — fresh-round mode for the RLM REPL.
//!
//! Runs a single fresh sub-agent child (no parent conversation history) for
//! the given task and retains a structured [`SubAgentReport`] across RLM
//! rounds. The child's intermediate dialogue never enters the main session
//! context; only the compact report is preserved.
//!
//! Implementation note: the heavy lifting lives in `crate::rlm::ralph`
//! (`run_fresh_round` / `RalphRoundStore`). When invoked from the REPL we
//! hand the model a precise instruction to spawn a *fresh* (`fork_context:
//! false`) child so the work is isolated; the ralph store is owned by the
//! RLM session and consulted for cross-round reporting. This command is the
//! documented integration point for the ralph fresh-round feature.

use crate::commands::traits::{CommandInfo, RegisterCommand};
use crate::localization::MessageId;
use crate::tui::app::{App, AppAction};

use super::CommandResult;

pub(in crate::commands) const COMMAND_INFO: CommandInfo = CommandInfo {
    name: "ralph",
    aliases: &[],
    usage: "/ralph <prompt>",
    description_id: MessageId::CmdRalphDescription,
};

pub(in crate::commands) struct RalphCmd;

impl RegisterCommand for RalphCmd {
    fn info() -> &'static CommandInfo {
        &COMMAND_INFO
    }

    fn execute(app: &mut App, arg: Option<&str>) -> CommandResult {
        ralph(app, arg)
    }
}

pub fn ralph(_app: &mut App, arg: Option<&str>) -> CommandResult {
    let prompt = match arg {
        Some(p) if !p.trim().is_empty() => p.trim().to_string(),
        _ => {
            return CommandResult::error(
                "Usage: /ralph <prompt>\n\n\
                 Runs a single FRESH sub-agent child (no parent history) for the task, \
                 then keeps a structured report across RLM rounds. The child's \
                 dialogue is never injected into the main session context."
                    .to_string(),
            );
        }
    };

    let message = format!(
        "Run a fresh-round ralph task. Spawn ONE fresh sub-agent child with `fork_context: false` \
         (do NOT inherit any parent conversation history) to work on this task: {prompt:?}. \
         When it finishes, report a concise structured summary (task, what it did, outcome, any \
         artifact references) so it can be retained as a cross-round ralph report. Do not paste the \
         child's raw transcript back into this session context.",
    );

    CommandResult::with_message_and_action(
        format!("Starting ralph fresh round for: {prompt}"),
        AppAction::SendMessage(message),
    )
}
