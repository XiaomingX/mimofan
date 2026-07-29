//! `/make-plan` command.

use crate::commands::CommandResult;
use crate::commands::traits::{CommandInfo, RegisterCommand};
use crate::localization::MessageId;
use crate::tui::app::{App, AppAction, AppMode};

pub(in crate::commands) const COMMAND_INFO: CommandInfo = CommandInfo {
    name: "make-plan",
    aliases: &["makeplan", "mp"],
    usage: "/make-plan <task_description>",
    description_id: MessageId::CmdMakePlanDescription,
};

pub(in crate::commands) struct MakePlanCmd;

impl RegisterCommand for MakePlanCmd {
    fn info() -> &'static CommandInfo {
        &COMMAND_INFO
    }

    fn execute(app: &mut App, arg: Option<&str>) -> CommandResult {
        let task = match arg.map(str::trim).filter(|s| !s.is_empty()) {
            Some(t) => t,
            None => return CommandResult::error("Usage: /make-plan <task_description>"),
        };

        // 1. Clear previous todo items
        if let Ok(mut todos) = app.todos.try_lock() {
            todos.clear();
        }

        // 2. Switch to Plan mode
        app.set_mode(AppMode::Plan);

        // 3. Return action to send prompt to LLM to write the plan
        let prompt = format!(
            "You are now in Plan mode. Please generate a detailed step-by-step implementation plan \
             using the checklist_write tool (or update_plan) for the following task:\n\n\
             Task: {}\n\n\
             Do NOT execute any code, shells, or write files. Only draft the plan.",
            task
        );

        CommandResult::with_message_and_action(
            "Switched to Plan mode. Generating plan...".to_string(),
            AppAction::SendMessage(prompt),
        )
    }
}
