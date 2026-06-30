//! User-defined slash commands from `~/.mimofan/commands/<name>.md` and
//! workspace-local `<workspace>/.mimofan/commands/<name>.md`.
//!
//! Users drop `.md` files into a commands directory and the filename
//! (without `.md` extension) becomes a slash command. When invoked via
//! `/name`, the file contents are sent as a user message.
//!
//! Files may include optional YAML-like frontmatter between `---` markers.
//! Supported fields are `description`, `argument-hint`, `allowed-tools`, and `pausable`.
//! Frontmatter is stripped before the command body is sent to the model.
//!
//! ## Precedence
//!
//! Workspace-local directories shadow user-global by name:
//!
//! 1. `<workspace>/.mimofan/commands/` (project-local, highest)
//! 2. `<workspace>/.deepseek/commands/`  (legacy project-local)
//! 3. `<workspace>/.claude/commands/`    (Claude Code interop)
//! 4. `<workspace>/.cursor/commands/`    (Cursor interop)
//! 5. `~/.mimofan/commands/`           (user-global)
//! 6. `~/.deepseek/commands/`            (legacy user-global)
//!
//! ## Permanent Role
//!
//! This module is the lower-level scanning, frontmatter parsing, and template
//! layer for [`super::user_registry::UserCommandRegistry`]. Runtime dispatch
//! lives in `user_registry.rs`; this file remains as the shared file I/O and
//! parsing boundary documented in `docs/architecture/command-dispatch.md`.

use std::path::{Path, PathBuf};

/// Path to the global user commands directory: `~/.mimofan/commands/`.
fn global_commands_dir() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("~"));
    home.join(".mimofan").join("commands")
}

fn legacy_global_commands_dir() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("~"));
    home.join(".deepseek").join("commands")
}

/// Return all candidate commands directories in precedence order.
pub(crate) fn commands_dirs(workspace: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(ws) = workspace {
        dirs.push(ws.join(".mimofan").join("commands"));
        dirs.push(ws.join(".deepseek").join("commands"));
        dirs.push(ws.join(".claude").join("commands"));
        dirs.push(ws.join(".cursor").join("commands"));
    }
    dirs.push(global_commands_dir());
    dirs.push(legacy_global_commands_dir());
    dirs
}

/// Scan a single commands directory for `.md` files and return
/// `(name, content)` pairs. Errors are silently skipped.
pub(crate) fn load_commands_from_dir(dir: &Path) -> Vec<(String, String)> {
    let mut commands: Vec<(String, String)> = Vec::new();

    if !dir.is_dir() {
        return commands;
    }

    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return commands,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let stem = match path.file_stem().and_then(|s| s.to_str()) {
            Some(stem) => stem.to_lowercase(),
            None => continue,
        };
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        commands.push((stem, content));
    }

    commands
}

/// Scan every candidate commands directory and return merged
/// `(name, content)` pairs. Workspace-local directories shadow
/// user-global by name — the first occurrence of a name wins.
///
/// Pass `None` for the workspace to scan only the global directory
/// (backward-compatible with callers that don't have workspace context).

pub(crate) fn parse_frontmatter(content: &str) -> (Vec<(String, String)>, &str) {
    let Some(first_line_end) = content.find('\n') else {
        return (Vec::new(), content);
    };
    let first = content[..first_line_end].trim_end_matches('\r');

    if first.trim().chars().all(|ch| ch == '-') && first.trim().len() >= 3 {
        let mut metadata = Vec::new();
        let mut offset = first_line_end + 1;
        let mut unclosed_body_start = None;
        for raw_line in content[offset..].split_inclusive('\n') {
            let line_start = offset;
            let line = raw_line.trim_end_matches(['\r', '\n']);
            offset += raw_line.len();
            let trimmed = line.trim();
            if unclosed_body_start.is_none() {
                if trimmed.chars().all(|ch| ch == '-') && trimmed.len() >= 3 {
                    let body = content[offset..].trim_start_matches(['\r', '\n']);
                    return (metadata, body);
                }
                if let Some((key, value)) = line.split_once(':') {
                    let key = key.trim().to_ascii_lowercase();
                    let raw_value = value.trim();
                    let value = if key == "allowed-tools" {
                        raw_value.to_string()
                    } else {
                        strip_matched_quotes(raw_value).to_string()
                    };
                    if !key.is_empty() {
                        metadata.push((key, value));
                    }
                } else if !trimmed.is_empty() {
                    unclosed_body_start = Some(line_start);
                }
            }
        }
        let body_start = unclosed_body_start.unwrap_or(content.len());
        let body = content[body_start..].trim_start_matches(['\r', '\n']);
        return (metadata, body);
    }

    (Vec::new(), content)
}

fn strip_matched_quotes(value: &str) -> &str {
    if let Some(stripped) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
        return stripped;
    }
    if let Some(stripped) = value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')) {
        return stripped;
    }
    value
}

pub(crate) fn parse_allowed_tools(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(|tool| {
            strip_matched_quotes(tool.trim())
                .trim()
                .to_ascii_lowercase()
        })
        .filter(|tool| !tool.is_empty())
        .collect()
}

/// Check if the input matches a user-defined command and return the
/// content as a `SendMessage` action.
///
/// The `input` should be the full command string including the `/`
/// prefix (e.g. `/mycmd` or `/mycmd with args`). Only exact matches
/// on the command name are considered (no partial/alias matching).
/// Substitute $1, $2, $ARGUMENTS placeholders in a command template.
pub(crate) fn apply_template(template: &str, args: &str) -> String {
    let positional: Vec<&str> = args.split_whitespace().collect();
    let mut result = template.replace("$ARGUMENTS", args);
    for (i, arg) in positional.iter().enumerate() {
        result = result.replace(&format!("${}", i + 1), arg);
    }
    result
}

#[cfg(test)]
mod tests {}
