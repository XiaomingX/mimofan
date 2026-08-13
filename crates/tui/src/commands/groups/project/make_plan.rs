//! `/make-plan` command.

use crate::commands::CommandResult;
use crate::commands::traits::{CommandInfo, RegisterCommand};
use crate::localization::MessageId;
use crate::tui::app::{App, AppAction, AppMode, HuntVerdict};

pub(in crate::commands) const COMMAND_INFO: CommandInfo = CommandInfo {
    name: "make-plan",
    aliases: &["makeplan", "mp"],
    usage: "/make-plan [--execute|-x] <task_description>",
    description_id: MessageId::CmdMakePlanDescription,
};

pub(in crate::commands) struct MakePlanCmd;

impl RegisterCommand for MakePlanCmd {
    fn info() -> &'static CommandInfo {
        &COMMAND_INFO
    }

    fn execute(app: &mut App, arg: Option<&str>) -> CommandResult {
        // Parse optional `--execute`/`-x` flag (plan-then-execute one-click).
        let (execute, task) = parse_make_plan_args(arg);

        let task = match task {
            Some(t) => t,
            None => {
                return CommandResult::error("Usage: /make-plan [--execute|-x] <task_description>");
            }
        };

        // 1. Clear previous todo items
        if let Ok(mut todos) = app.todos.try_lock() {
            todos.clear();
        }

        if execute {
            // Plan-then-execute in one shot (CodeBuddy auto-plan/do parity).
            // The model first drafts the plan via checklist_write, then runs
            // every step and closes the goal with `goal_update complete`,
            // reusing the `/goal` continuation pipeline for autonomous stepping.
            app.hunt.quarry = Some(task.to_string());
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

            app.set_mode(AppMode::Agent);

            let prompt = format!(
                "You are now in Agent mode. First draft a detailed step-by-step implementation \
                 plan for the task using the checklist_write tool (or update_plan). Then \
                 immediately execute every planned step in order, updating each step's status \
                 to completed via checklist_write as you finish it.\n\n\
                 Task: {}\n\n\
                 When ALL steps are completed, call `goal_update` with `status: \"complete\"`, \
                 concrete evidence of completion, and \
                 `verification: {{\"status\":\"passed\",\"check\":\"all steps done\",\"summary\":\"...\"}}` \
                 to end the run. If you hit a real blocker, call `goal_update` with \
                 `status: \"blocked\"` and the blocker instead.",
                task
            );

            CommandResult::with_message_and_action(
                "Switched to Agent mode. Planning and executing end-to-end...".to_string(),
                AppAction::SendMessage(prompt),
            )
        } else {
            // 2. Plan mode only (no execution).
            app.set_mode(AppMode::Plan);

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
}

/// Parse `/make-plan` arguments, returning `(execute_flag, task)`.
fn parse_make_plan_args(arg: Option<&str>) -> (bool, Option<String>) {
    let Some(arg) = arg else {
        return (false, None);
    };
    let mut execute = false;
    let mut task_parts: Vec<&str> = Vec::new();
    for tok in arg.split_whitespace() {
        match tok {
            "--execute" | "-x" => execute = true,
            other => task_parts.push(other),
        }
    }
    let task = task_parts.join(" ").trim().to_string();
    (execute, if task.is_empty() { None } else { Some(task) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plan_only() {
        let (exec, task) = parse_make_plan_args(Some("add a login form"));
        assert!(!exec);
        assert_eq!(task, Some("add a login form".to_string()));
    }

    #[test]
    fn parses_execute_long_flag() {
        let (exec, task) = parse_make_plan_args(Some("--execute refactor the parser"));
        assert!(exec);
        assert_eq!(task, Some("refactor the parser".to_string()));
    }

    #[test]
    fn parses_execute_short_flag() {
        let (exec, task) = parse_make_plan_args(Some("-x ship the feature"));
        assert!(exec);
        assert_eq!(task, Some("ship the feature".to_string()));
    }

    #[test]
    fn parses_flag_after_task() {
        let (exec, task) = parse_make_plan_args(Some("build x -x"));
        assert!(exec);
        assert_eq!(task, Some("build x".to_string()));
    }

    #[test]
    fn empty_arg_yields_none() {
        assert_eq!(parse_make_plan_args(None), (false, None));
        assert_eq!(parse_make_plan_args(Some("   ")), (false, None));
    }
}
