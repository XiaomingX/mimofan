//! `/do` command.

use crate::commands::CommandResult;
use crate::commands::traits::{CommandInfo, RegisterCommand};
use crate::localization::MessageId;
use crate::tools::todo::TodoStatus;
use crate::tui::app::{App, AppAction, AppMode};

pub(in crate::commands) const COMMAND_INFO: CommandInfo = CommandInfo {
    name: "do",
    aliases: &[],
    usage: "/do [step_id|all|next]",
    description_id: MessageId::CmdDoDescription,
};

pub(in crate::commands) struct DoCmd;

impl RegisterCommand for DoCmd {
    fn info() -> &'static CommandInfo {
        &COMMAND_INFO
    }

    fn execute(app: &mut App, arg: Option<&str>) -> CommandResult {
        let target = arg
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("next");

        let mut todos_guard = match app.todos.try_lock() {
            Ok(g) => g,
            Err(_) => {
                return CommandResult::error(
                    "Checklist tool is currently locked. Please try again.",
                );
            }
        };

        let snapshot = todos_guard.snapshot();
        if snapshot.items.is_empty() {
            return CommandResult::error("No checklist steps found. Please use /make-plan first.");
        }

        if target == "all" {
            let pending_items: Vec<_> = snapshot
                .items
                .iter()
                .filter(|item| item.status != TodoStatus::Completed)
                .collect();
            if pending_items.is_empty() {
                return CommandResult::message("All checklist steps are already completed!");
            }

            let first_item = pending_items[0];
            drop(todos_guard);

            app.set_mode(AppMode::Agent);

            let prompt = format!(
                "You are now in Agent mode. Please execute all remaining pending checklist steps. \
                 Start by executing the next step:\n\n\
                 Step {}: {}\n\n\
                 Update the status of each checklist step using checklist_write as you complete them.",
                first_item.id, first_item.content
            );

            CommandResult::with_message_and_action(
                "Switched to Agent mode. Executing all pending steps...".to_string(),
                AppAction::SendMessage(prompt),
            )
        } else if target == "next" {
            let next_item = snapshot
                .items
                .iter()
                .find(|item| item.status != TodoStatus::Completed);

            let Some(item) = next_item else {
                return CommandResult::message("All checklist steps are already completed!");
            };

            let item_id = item.id;
            let item_content = item.content.clone();
            todos_guard.update_status(item_id, TodoStatus::InProgress);
            drop(todos_guard);

            app.set_mode(AppMode::Agent);

            let prompt = format!(
                "You are now in Agent mode. Please execute Step {} from the checklist:\n\n\
                 Step {}: {}\n\n\
                 Once completed, update its status to completed in the checklist.",
                item_id, item_id, item_content
            );

            CommandResult::with_message_and_action(
                format!("Switched to Agent mode. Executing Step {}...", item_id),
                AppAction::SendMessage(prompt),
            )
        } else {
            // Parse numerical step ID
            let id: u32 = match target.parse() {
                Ok(num) => num,
                Err(_) => return CommandResult::error("Usage: /do [step_id|all|next]"),
            };

            let item = snapshot.items.iter().find(|item| item.id == id);
            let Some(item) = item else {
                return CommandResult::error(format!(
                    "Step {} not found in the current checklist.",
                    id
                ));
            };

            if item.status == TodoStatus::Completed {
                return CommandResult::message(format!("Step {} is already completed!", id));
            }

            let item_content = item.content.clone();
            todos_guard.update_status(id, TodoStatus::InProgress);
            drop(todos_guard);

            app.set_mode(AppMode::Agent);

            let prompt = format!(
                "You are now in Agent mode. Please execute Step {} from the checklist:\n\n\
                 Step {}: {}\n\n\
                 Once completed, update its status to completed in the checklist.",
                id, id, item_content
            );

            CommandResult::with_message_and_action(
                format!("Switched to Agent mode. Executing Step {}...", id),
                AppAction::SendMessage(prompt),
            )
        }
    }
}
