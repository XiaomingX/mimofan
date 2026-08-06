//! `/memory` slash command — inspect and manage the categorized memory store.
//!
//! When the user-memory feature is opted-in (`[memory] enabled = true` in
//! config or `MIMOFAN_MEMORY=on` in the environment), memory lives in a
//! directory (`~/.mimofan/memory/` by default) holding `MEMORY.md` (the
//! always-injected index) plus one file per category
//! (`user.md` / `feedback.md` / `project.md` / `reference.md`).
//!
//! Subcommands:
//! - `/memory` — show the index (`MEMORY.md`) and available categories
//! - `/memory show [category]` — show a category file (or all non-empty)
//! - `/memory path` — show the memory directory
//! - `/memory clear [category]` — clear a category (or the whole store)
//! - `/memory edit [category]` — print an editor command for the index or a
//!   category file
//! - `/memory help` — this help
//!
//! Quick capture: `# foo` from the composer appends to the default category
//! (`project`); `# user foo` routes to `user.md`. The `remember` tool writes
//! with an explicit category.

use std::fs;

use super::CommandResult;
use crate::memory::{
    CATEGORIES, category_path, index_path, is_category, load_index, read_category, write_index,
};
use crate::tui::app::App;

const MEMORY_USAGE: &str = "/memory [show [category]|path|clear [category]|edit [category]|help]";

fn memory_help(dir: &std::path::Path) -> String {
    format!(
        "Inspect or manage your categorized user memory.\n\n\
         Usage: {MEMORY_USAGE}\n\n\
         Memory directory: {}\n\n\
         Subcommands:\n\
           /memory                Show the index and available categories\n\
           /memory show [cat]     Show a category file (or every non-empty one)\n\
           /memory path           Print the memory directory\n\
           /memory clear [cat]    Clear a category (or the whole store if omitted)\n\
           /memory edit [cat]     Print an editor command for the index or a category\n\
           /memory help           Show this help\n\n\
         Categories: {}\n\
         Quick capture: `# foo` appends to `project`; `# user foo` to `user`.",
        dir.display(),
        CATEGORIES.join(", "),
    )
}

/// Count categories that currently have non-empty content.
fn populated_categories(dir: &std::path::Path) -> Vec<&'static str> {
    CATEGORIES
        .iter()
        .copied()
        .filter(|cat| read_category(dir, cat).is_some())
        .collect()
}

pub fn memory(app: &mut App, arg: Option<&str>) -> CommandResult {
    if !app.use_memory {
        return CommandResult::error(
            "user memory is disabled. Enable with `[memory] enabled = true` in `~/.mimofan/config.toml` or `MIMOFAN_MEMORY=on` in your environment, then restart the TUI.",
        );
    }

    let dir = app.memory_dir.clone();
    let sub = arg.unwrap_or("show").trim();

    match sub {
        "path" => CommandResult::message(dir.display().to_string()),
        "help" => CommandResult::message(memory_help(&dir)),
        "" | "show" => {
            let populated = populated_categories(&dir);
            let index = load_index(&dir);
            let mut body = format!("{}\n", dir.display());
            if let Some(index) = index {
                body.push('\n');
                body.push_str(index.trim_end());
                body.push('\n');
            } else {
                body.push_str("\n(index is empty — add via `# foo` or the `remember` tool)\n");
            }
            if populated.is_empty() {
                body.push_str("\nNo populated categories yet.\n");
            } else {
                body.push_str(&format!(
                    "\nPopulated categories: {}\n",
                    populated.join(", ")
                ));
            }
            CommandResult::message(body)
        }
        s if s.starts_with("show ") => {
            let cat = s["show ".len()..].trim();
            if cat.is_empty() {
                // No category: show every non-empty category concatenated.
                let populated = populated_categories(&dir);
                if populated.is_empty() {
                    return CommandResult::message(format!(
                        "{}\n\n(no populated categories)",
                        dir.display()
                    ));
                }
                let mut body = String::new();
                for c in populated {
                    if let Some(content) = read_category(&dir, c) {
                        body.push_str(&format!("=== {c}.md ===\n{content}\n"));
                    }
                }
                CommandResult::message(body)
            } else if is_category(cat) {
                match read_category(&dir, cat) {
                    Some(content) => CommandResult::message(format!(
                        "{}\n\n{}",
                        category_path(&dir, cat).display(),
                        content.trim_end()
                    )),
                    None => CommandResult::message(format!(
                        "{} is empty (no entries yet)",
                        category_path(&dir, cat).display()
                    )),
                }
            } else {
                CommandResult::error(format!(
                    "unknown category `{cat}`. Expected one of: {}",
                    CATEGORIES.join(", ")
                ))
            }
        }
        s if s == "edit" || s.starts_with("edit ") => {
            let cat = s.strip_prefix("edit").unwrap_or("").trim();
            let target = if cat.is_empty() {
                index_path(&dir)
            } else if is_category(cat) {
                category_path(&dir, cat)
            } else {
                return CommandResult::error(format!(
                    "unknown category `{cat}`. Expected one of: {}",
                    CATEGORIES.join(", ")
                ));
            };
            CommandResult::message(format!(
                "to edit, run:\n\n  ${{VISUAL:-${{EDITOR:-vi}}}} {}",
                target.display()
            ))
        }
        s if s.starts_with("clear") => {
            let rest = s["clear".len()..].trim();
            if rest.is_empty() {
                // Clear the whole store: empty every category, then rebuild
                // the (now-empty) index.
                for cat in CATEGORIES {
                    let _ = fs::write(category_path(&dir, cat), "");
                }
                let _ = write_index(&dir);
                CommandResult::message(format!("memory cleared: {}", dir.display()))
            } else if is_category(rest) {
                match fs::write(category_path(&dir, rest), "") {
                    Ok(()) => {
                        let _ = write_index(&dir);
                        CommandResult::message(format!("cleared {}.md (index refreshed)", rest))
                    }
                    Err(err) => CommandResult::error(format!("failed to clear {}.md: {err}", rest)),
                }
            } else {
                CommandResult::error(format!(
                    "unknown category `{rest}`. Expected one of: {}",
                    CATEGORIES.join(", ")
                ))
            }
        }
        _ => CommandResult::error(format!(
            "unknown subcommand `{sub}`. Try `/memory help`.\n\n{}",
            memory_help(&dir)
        )),
    }
}
