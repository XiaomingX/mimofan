//! Task commands: add/list/show/cancel

use crate::tui::app::{App, AppAction};

use super::CommandResult;

pub fn task(_app: &mut App, args: Option<&str>) -> CommandResult {
    let raw = args.unwrap_or("").trim();
    if raw.is_empty() || raw.eq_ignore_ascii_case("list") {
        return CommandResult::action(AppAction::TaskList);
    }

    let mut parts = raw.splitn(2, char::is_whitespace);
    let action = parts.next().unwrap_or("").to_ascii_lowercase();
    let remainder = parts.next().map(str::trim).filter(|s| !s.is_empty());

    match action.as_str() {
        "add" => {
            let Some(prompt) = remainder else {
                return CommandResult::error("Usage: /task add <prompt>");
            };
            CommandResult::action(AppAction::TaskAdd {
                prompt: prompt.to_string(),
            })
        }
        "list" => CommandResult::action(AppAction::TaskList),
        "show" => {
            let Some(id) = remainder else {
                return CommandResult::error("Usage: /task show <id>");
            };
            CommandResult::action(AppAction::TaskShow { id: id.to_string() })
        }
        "cancel" | "stop" => {
            let Some(id) = remainder else {
                return CommandResult::error("Usage: /task cancel <id>");
            };
            CommandResult::action(AppAction::TaskCancel { id: id.to_string() })
        }
        _ => CommandResult::error("Usage: /task [add <prompt>|list|show <id>|cancel <id>]"),
    }
}

#[cfg(test)]
mod tests {}
