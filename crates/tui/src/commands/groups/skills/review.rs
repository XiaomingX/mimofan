//! Review command: activate review skill and send a target immediately.

use crate::skills::{SkillRegistry, default_skills_dir};
use crate::tui::app::{App, AppAction};
use crate::tui::history::HistoryCell;

use super::CommandResult;

pub fn review(app: &mut App, args: Option<&str>) -> CommandResult {
    let target = args.unwrap_or("").trim();
    if target.is_empty() {
        return CommandResult::error("Usage: /review <target>");
    }

    let skills_dir = app.skills_dir.clone();
    let registry = SkillRegistry::discover(&skills_dir);
    let mut skill = registry.get("review").cloned();

    let global_dir = default_skills_dir();
    if skill.is_none() && global_dir != skills_dir {
        let registry = SkillRegistry::discover(&global_dir);
        skill = registry.get("review").cloned();
    }

    let instruction = match skill {
        Some(s) => {
            format!(
                include_str!("../../../prompts/skill_loader.md"),
                skill_name = s.name,
                skill_body = s.body
            )
        }
        None => {
            format!(
                include_str!("../../../prompts/skill_loader.md"),
                skill_name = "review",
                skill_body = include_str!("../../../prompts/review_skill.md")
            )
        }
    };

    app.add_message(HistoryCell::System {
        content: "Activated code review skill (running scan)...".to_string(),
    });
    app.active_skill = Some(instruction);

    CommandResult::action(AppAction::SendMessage(target.to_string()))
}
