//! Slash command registry and dispatch system
//!
//! This module provides a modular command system inspired by Codex-rs.
//! Commands are organized by category and dispatched through a central strategy
//! registry. Built-in handlers live in group-owned areas under [`groups`]; this
//! module keeps registry construction, user-command precedence, and the
//! fall-through behaviour.

mod groups;
mod plugins;
pub mod traits;
pub mod user_commands;
pub mod user_registry;

use std::sync::OnceLock;

pub use traits::CommandInfo;

// Long-standing public paths that predate the group layout.
pub use groups::project::share;

// Voice capture plumbing shared with the hotbar and the UI event loop.
pub use groups::core::voice;

use crate::tui::app::{App, AppAction};

/// Result of executing a command
#[derive(Debug, Clone)]
pub struct CommandResult {
    /// Optional message to display to the user
    pub message: Option<String>,
    /// Optional action for the app to take
    pub action: Option<AppAction>,
    /// Whether the command failed.
    pub is_error: bool,
}

impl CommandResult {
    /// Create an empty result (command succeeded with no output)
    pub fn ok() -> Self {
        Self {
            message: None,
            action: None,
            is_error: false,
        }
    }

    /// Create a result with just a message
    pub fn message(msg: impl Into<String>) -> Self {
        Self {
            message: Some(msg.into()),
            action: None,
            is_error: false,
        }
    }

    /// Create a result with an action
    pub fn action(action: AppAction) -> Self {
        Self {
            message: None,
            action: Some(action),
            is_error: false,
        }
    }

    /// Create a result with both message and action
    pub fn with_message_and_action(msg: impl Into<String>, action: AppAction) -> Self {
        Self {
            message: Some(msg.into()),
            action: Some(action),
            is_error: false,
        }
    }

    /// Create an error message result
    pub fn error(msg: impl Into<String>) -> Self {
        Self {
            message: Some(format!("Error: {}", msg.into())),
            action: None,
            is_error: true,
        }
    }
}

static REGISTRY: OnceLock<traits::CommandRegistry> = OnceLock::new();

fn build_registry() -> traits::CommandRegistry {
    let mut registry = traits::CommandRegistry::empty();
    for group in groups::all_command_groups() {
        registry.register_group(group);
    }
    registry
}

pub fn registry() -> &'static traits::CommandRegistry {
    REGISTRY.get_or_init(build_registry)
}

pub fn command_infos() -> Vec<&'static CommandInfo> {
    registry().infos()
}

pub fn get_command_info(name: &str) -> Option<&'static CommandInfo> {
    registry().get_info(name)
}

/// Execute a slash command
pub fn execute(cmd: &str, app: &mut App) -> CommandResult {
    let trimmed = cmd.trim();

    // `$skillname` is a backward-compatible alias for `/skill skillname`.
    // Resolve it early so skills can be loaded with the `$` prefix.
    if let Some(skill_input) = trimmed.strip_prefix('$') {
        let skill_input = skill_input.trim_start();
        if skill_input.is_empty() {
            return CommandResult::error(
                "Type a skill name after $. For example: $getting-started",
            );
        }
        let parts: Vec<&str> = skill_input.splitn(2, char::is_whitespace).collect();
        let skill_name = parts.first().copied().unwrap_or("");
        let arg = parts
            .get(1)
            .map(|value| value.trim())
            .filter(|value| !value.is_empty());
        if let Some(result) = groups::skills::run_skill_by_name(app, skill_name, arg) {
            return result;
        }
        return CommandResult::error(format!(
            "Unknown skill: ${skill_name}. Type /skills to see installed skills."
        ));
    }

    let parts: Vec<&str> = trimmed.splitn(2, char::is_whitespace).collect();
    let command = parts
        .first()
        .copied()
        .unwrap_or_default()
        .trim_start_matches('/')
        .to_ascii_lowercase();
    let arg = parts
        .get(1)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty());

    // Check user-defined commands FIRST so they can override built-ins.
    if let Some(result) = user_registry::try_dispatch(app, trimmed) {
        return result;
    }

    // Permanent backward-compatible aliases. They predate the group-owned
    // registry and remain documented in docs/architecture/command-dispatch.md.
    match command.as_str() {
        "jihua" => {
            return groups::config::dispatch(app, "jihua", arg).unwrap_or_else(|| {
                CommandResult::error("The /jihua alias could not be dispatched.")
            });
        }
        "zidong" => {
            return groups::config::dispatch(app, "zidong", arg).unwrap_or_else(|| {
                CommandResult::error("The /zidong alias could not be dispatched.")
            });
        }
        "slop" | "canzha" => {
            return groups::config::dispatch(app, "debt", arg).unwrap_or_else(|| {
                CommandResult::error("The /debt command could not be dispatched.")
            });
        }
        _ => {}
    }

    if let Some(command_object) = registry().get(command.as_str()) {
        return command_object.execute(app, arg);
    }

    match command.as_str() {
        // Permanent legacy migration hints. These are deliberately excluded
        // from registry/autocomplete and only appear when users type old names.
        "set" => CommandResult::error(
            "The /set command was retired. Use /config to edit settings and /settings to inspect current values.",
        ),
        "deepseek" => CommandResult::error(
            "The /deepseek command was renamed. Use /links (aliases: /dashboard, /api).",
        ),

        _ => {
            // Third source: skills (lowest precedence after native and user-config).
            // Try to run a skill whose name matches the command.
            if let Some(result) = groups::skills::run_skill_by_name(app, command.as_str(), arg) {
                return result;
            }
            let suggestions = suggest_command_names(command.as_str(), 3);
            if suggestions.is_empty() {
                CommandResult::error(format!(
                    "Unknown command: /{command}. Type /help for available commands."
                ))
            } else {
                let list = suggestions
                    .into_iter()
                    .map(|name| format!("/{name}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                CommandResult::error(format!(
                    "Unknown command: /{command}. Did you mean: {list}? Type /help for available commands."
                ))
            }
        }
    }
}

/// Update a configuration value programmatically (used by interactive UI views).
pub fn set_config_value(app: &mut App, key: &str, value: &str, persist: bool) -> CommandResult {
    groups::config::config::set_config_value(app, key, value, persist)
}

pub fn switch_mode(app: &mut App, mode: crate::tui::app::AppMode) -> String {
    groups::config::config::switch_mode(app, mode)
}

fn edit_distance(a: &str, b: &str) -> usize {
    if a == b {
        return 0;
    }
    if a.is_empty() {
        return b.chars().count();
    }
    if b.is_empty() {
        return a.chars().count();
    }

    let b_chars: Vec<char> = b.chars().collect();
    let mut previous: Vec<usize> = (0..=b_chars.len()).collect();
    let mut current = vec![0usize; b_chars.len() + 1];

    for (i, a_ch) in a.chars().enumerate() {
        current[0] = i + 1;
        for (j, b_ch) in b_chars.iter().enumerate() {
            let cost = if a_ch == *b_ch { 0 } else { 1 };
            let delete = previous[j + 1] + 1;
            let insert = current[j] + 1;
            let substitute = previous[j] + cost;
            current[j + 1] = delete.min(insert).min(substitute);
        }
        std::mem::swap(&mut previous, &mut current);
    }

    previous[b_chars.len()]
}

fn suggest_command_names(input: &str, limit: usize) -> Vec<String> {
    let query = input.trim().to_ascii_lowercase();
    if query.is_empty() || limit == 0 {
        return Vec::new();
    }

    let mut scored: Vec<(u8, usize, String)> = Vec::new();
    for command in registry().infos() {
        let mut best: Option<(u8, usize)> = None;
        for candidate in std::iter::once(command.name).chain(command.aliases.iter().copied()) {
            let prefix_match = candidate.starts_with(&query) || query.starts_with(candidate);
            let contains_match = candidate.contains(&query) || query.contains(candidate);
            let distance = edit_distance(candidate, &query);
            let close_typo = distance <= 2;
            if !(prefix_match || contains_match || close_typo) {
                continue;
            }

            let rank = if prefix_match {
                0
            } else if contains_match {
                1
            } else {
                2
            };

            match best {
                Some((best_rank, best_distance))
                    if rank > best_rank || (rank == best_rank && distance >= best_distance) => {}
                _ => best = Some((rank, distance)),
            }
        }

        if let Some((rank, distance)) = best {
            scored.push((rank, distance, command.name.to_string()));
        }
    }

    scored.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| a.1.cmp(&b.1))
            .then_with(|| a.2.cmp(&b.2))
    });
    scored
        .into_iter()
        .take(limit)
        .map(|(_, _, name)| name)
        .collect()
}

#[cfg(test)]
mod tests {}
