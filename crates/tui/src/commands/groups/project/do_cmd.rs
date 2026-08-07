//! `/do` command.

use crate::commands::CommandResult;
use crate::commands::traits::{CommandInfo, RegisterCommand};
use crate::localization::MessageId;
use crate::tools::todo::TodoStatus;
use crate::tui::app::{App, AppAction, AppMode, HuntVerdict};

pub(in crate::commands) const COMMAND_INFO: CommandInfo = CommandInfo {
    name: "do",
    aliases: &["run"],
    usage: "/do [step_id|all|next|auto]",
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
        } else if target == "auto" {
            // `/do auto` — autonomous end-to-end execution of the whole plan.
            //
            // Reuses the existing `/goal` continuation pipeline: the model first
            // calls `create_goal` to stand up a persistent objective ("execute
            // the plan"), then works through every pending checklist step, and
            // finally calls `update_goal` with `status: "complete"` once all
            // steps are done. The engine's goal continuation re-dispatches turns
            // automatically between steps — no manual `/do next` needed. This is
            // the parity path for CodeBuddy's one-click "make plan + do it".
            let pending_items: Vec<_> = snapshot
                .items
                .iter()
                .filter(|item| item.status != TodoStatus::Completed)
                .collect();
            if pending_items.is_empty() {
                return CommandResult::message("All checklist steps are already completed!");
            }

            // Seed the UI-side hunt state so status surfaces and the loop/goal
            // controls stay consistent; the engine truth (GoalState) is created
            // by the model's `create_goal` call on the first turn.
            let objective = "Execute the full plan: work through every pending checklist step until all are complete.".to_string();
            app.hunt.quarry = Some(objective.clone());
            app.hunt.token_budget = None;
            app.hunt.tokens_used = 0;
            app.hunt.time_used_seconds = 0;
            app.hunt.continuation_count = 0;
            app.hunt.started_at = Some(std::time::Instant::now());
            app.hunt.verdict = HuntVerdict::Hunting;
            app.hunt.stop_condition = None;
            app.hunt.max_rounds = None;
            app.hunt.checkpoint_each_round = false;
            app.hunt.is_loop = false;

            let pending_summary: String = pending_items
                .iter()
                .map(|item| format!("  - [{}] {}", item.id, item.content))
                .collect::<Vec<_>>()
                .join("\n");
            // `snapshot` (and thus `pending_items`) is owned data, so the todo
            // guard can be released before we mutate `app` for the mode switch.
            drop(todos_guard);

            app.set_mode(AppMode::Agent);

            let prompt = format!(
                "You are now in Agent mode and running the plan autonomously. \
                 First call `create_goal` with objective \"{}\" to stand up a persistent goal. \
                 Then execute every pending checklist step in order, updating each step's status to completed via checklist_write as you finish it.\n\n\
                 Pending steps:\n{}\n\n\
                 When ALL steps are completed, call `update_goal` with `status: \"complete\"`, \
                 concrete evidence of completion, and `verification: {{\"status\":\"passed\",\"check\":\"all steps done\",\"summary\":\"...\"}}` to end the run. \
                 If you hit a real blocker, call `update_goal` with `status: \"blocked\"` and the blocker instead.",
                objective, pending_summary
            );

            CommandResult::with_message_and_action(
                "Switched to Agent mode. Autonomous plan execution started (create_goal → run all steps → update_goal complete).".to_string(),
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
