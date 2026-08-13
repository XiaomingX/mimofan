//! `/research` command — autonomous deep-dive research with memory
//! sedimentation (claude-mem parity).
//!
//! Unlike `/goal` (a persistent objective the user pursues) or `/loop` (a
//! prompt repeated until a stop condition), `/research` is a focused
//! investigation: the model explores the codebase / topic, reasons about it,
//! and — crucially — **persists what it learned** to long-term memory via the
//! `remember` (file-based) and `remember_vector` (semantic) tools so the
//! knowledge survives the session. This mirrors claude-mem's "capture the
//! task process and extract structured knowledge" behavior, but exposed as an
//! explicit, user-triggered command rather than an implicit background sink.

use crate::commands::CommandResult;
use crate::commands::traits::{CommandInfo, RegisterCommand};
use crate::localization::MessageId;
use crate::tui::app::{App, AppAction, AppMode};

pub(in crate::commands) const COMMAND_INFO: CommandInfo = CommandInfo {
    name: "research",
    aliases: &["memo", "investigate"],
    usage: "/research <topic>",
    description_id: MessageId::CmdResearchDescription,
};

pub(in crate::commands) struct ResearchCmd;

impl RegisterCommand for ResearchCmd {
    fn info() -> &'static CommandInfo {
        &COMMAND_INFO
    }

    fn execute(app: &mut App, arg: Option<&str>) -> CommandResult {
        research(app, arg)
    }
}

/// Dispatch a `/research <topic>` invocation.
fn research(app: &mut App, arg: Option<&str>) -> CommandResult {
    let topic = match arg.map(str::trim).filter(|s| !s.is_empty()) {
        Some(t) => t,
        None => {
            return CommandResult::error(
                "Usage: /research <topic>\n\n\
                 Investigates <topic> across the codebase and persists the findings \
                 to long-term memory (use /recall to retrieve later).",
            );
        }
    };

    // Research runs in Agent mode so the model can read files and call tools.
    app.set_mode(AppMode::Agent);

    let prompt = format!(
        "You are now in Agent mode running a focused research task.\n\n\
         Topic: {}\n\n\
         Investigate this topic thoroughly: read the relevant code, follow the \
         call paths, and form a clear, structured understanding. Prefer primary \
         sources (the actual code) over assumptions.\n\n\
         As you learn durable facts, decisions, gotchas, and non-obvious \
         invariants, persist them to long-term memory so they survive this \
         session:\n\
         - Use `remember` for a concise file-based note (bullet or short prose).\n\
         - Use `remember_vector` for knowledge you may want to retrieve later by \
         semantic similarity (e.g. \"how does X decide Y\", \"where is the Z \
         boundary handled\").\n\n\
         When the investigation is complete, give the user a concise summary of \
         the findings and list what you saved to memory.",
        topic
    );

    CommandResult::with_message_and_action(
        format!(
            "Switched to Agent mode. Researching \"{topic}\" and persisting findings to memory..."
        ),
        AppAction::SendMessage(prompt),
    )
}
