//! Review command: activate review skill and send a target immediately.

use crate::skills::{SkillRegistry, default_skills_dir};
use crate::tui::app::{App, AppAction};
use crate::tui::history::HistoryCell;

use super::CommandResult;

fn warnings_suffix(registry: &SkillRegistry) -> String {
    if registry.warnings().is_empty() {
        return String::new();
    }

    format!("\n\nWarnings:\n- {}", registry.warnings().join("\n- "))
}

pub fn review(app: &mut App, args: Option<&str>) -> CommandResult {
    let target = args.unwrap_or("").trim();
    if target.is_empty() {
        return CommandResult::error("Usage: /review <target>");
    }

    let skills_dir = app.skills_dir.clone();
    let registry = SkillRegistry::discover(&skills_dir);
    let mut warnings = warnings_suffix(&registry);
    let mut skill = registry.get("review").cloned();

    let global_dir = default_skills_dir();
    if skill.is_none() && global_dir != skills_dir {
        let registry = SkillRegistry::discover(&global_dir);
        if warnings.is_empty() {
            warnings = warnings_suffix(&registry);
        } else if !registry.warnings().is_empty() {
            warnings.push_str(&format!("\n- {}", registry.warnings().join("\n- ")));
        }
        skill = registry.get("review").cloned();
    }

    let skill = match skill {
        Some(skill) => skill,
        None => {
            let global_display = global_dir.display();
            return CommandResult::error(format!(
                "Review skill not found in {} or {}. Create ~/.mimofanfan/skills/review/SKILL.md.{}",
                skills_dir.display(),
                global_display,
                warnings
            ));
        }
    };

    let instruction = format!(
        include_str!("../../../prompts/skill_loader.md"),
        skill_name = skill.name,
        skill_body = skill.body
    );

    app.add_message(HistoryCell::System {
        content: format!("Activated skill: {}\n\n{}", skill.name, skill.description),
    });
    app.active_skill = Some(instruction);

    CommandResult::action(AppAction::SendMessage(target.to_string()))
}

#[cfg(test)]
mod tests {}
