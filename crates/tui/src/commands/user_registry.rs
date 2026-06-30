//! Dedicated registry for user-defined markdown slash commands.
//!
//! This module owns the user-command boundary. Built-in command metadata and
//! dispatch remain in the normal command registry; user commands are loaded
//! from markdown files into this registry and are attempted before built-ins.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};
use std::time::{Duration, SystemTime};

use crate::tui::app::{App, AppAction, HuntVerdict};

use super::CommandResult;
use super::user_commands;

static USER_COMMAND_REGISTRY: OnceLock<RwLock<UserCommandRegistryState>> = OnceLock::new();

#[derive(Debug, Clone, Default)]
struct UserCommandRegistryState {
    initialized: bool,
    workspace: Option<PathBuf>,
    command_dirs_snapshot: Vec<CommandDirSnapshot>,
    registry: UserCommandRegistry,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CommandDirSnapshot {
    path: PathBuf,
    modified: Option<SystemTime>,
    files: Vec<CommandFileSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CommandFileSnapshot {
    path: PathBuf,
    modified: Option<SystemTime>,
    len: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserCommandMetadata {
    pub name: String,
    pub body: String,
    pub description: Option<String>,
    pub argument_hint: Option<String>,
    pub allowed_tools: Option<Vec<String>>,
    pub pausable: bool,
    pub aliases: Vec<String>,
    pub hidden: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadError {
    pub path: PathBuf,
    pub message: String,
}

#[derive(Debug, Clone, Default)]
pub struct UserCommandRegistry {
    commands: HashMap<String, UserCommandMetadata>,
    aliases: HashMap<String, String>,
    load_errors: Vec<LoadError>,
    invalid_commands: HashMap<String, String>,
}

impl UserCommandRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn load(workspace: Option<&Path>) -> Self {
        // The user_commands module is the permanent lower-level file scanning
        // and parsing boundary; this registry owns metadata, shadowing, and
        // dispatch. See docs/architecture/command-dispatch.md.
        Self::load_from_paths(&user_commands::commands_dirs(workspace))
    }

    pub(crate) fn load_from_paths(paths: &[PathBuf]) -> Self {
        let mut loaded = Vec::new();
        let mut seen = HashSet::new();
        let mut registry = Self::new();

        for dir in paths {
            for (name, content) in user_commands::load_commands_from_dir(dir) {
                let canonical = normalize_name(&name);
                if seen.insert(canonical.clone()) {
                    loaded.push((name, content, dir.join(format!("{canonical}.md"))));
                } else {
                    registry.record_load_error(
                        dir.join(format!("{canonical}.md")),
                        format!(
                            "User command '/{canonical}' is defined more than once; using the first definition"
                        ),
                    );
                }
            }
        }
        loaded.sort_by(|a, b| a.0.cmp(&b.0));
        registry.load_from_entries(loaded);
        registry
    }

    fn load_from_entries(&mut self, commands: Vec<(String, String, PathBuf)>) {
        let canonical_names = commands
            .iter()
            .map(|(name, _, _)| normalize_name(name))
            .collect::<HashSet<_>>();

        for (name, content, path) in commands {
            let (metadata, errors) = parse_metadata(name, &content, &path);
            for error in errors {
                self.record_load_error(error.path.clone(), error.message.clone());
                self.invalid_commands
                    .entry(metadata.name.clone())
                    .or_insert(error.message);
            }

            if self.commands.contains_key(&metadata.name) {
                self.record_load_error(
                    path.clone(),
                    format!(
                        "User command '/{}' is defined more than once; using the first definition",
                        metadata.name
                    ),
                );
                continue;
            }

            for alias in &metadata.aliases {
                let alias = alias.to_ascii_lowercase();
                if canonical_names.contains(&alias) {
                    self.record_load_error(
                        path.clone(),
                        format!(
                            "User command alias '/{alias}' for '/{}' duplicates canonical user command '/{alias}'; ignoring this alias",
                            metadata.name
                        ),
                    );
                    continue;
                }
                if let Some(existing) = self.aliases.get(&alias) {
                    self.record_load_error(
                        path.clone(),
                        format!(
                            "User command alias '/{alias}' for '/{}' duplicates user command '/{existing}'; using the first alias definition",
                            metadata.name
                        ),
                    );
                    continue;
                }
                self.aliases.insert(alias, metadata.name.clone());
            }

            self.commands.insert(metadata.name.clone(), metadata);
        }
    }

    fn record_load_error(&mut self, path: PathBuf, message: String) {
        self.load_errors.push(LoadError { path, message });
    }

    pub fn get(&self, name: &str) -> Option<&UserCommandMetadata> {
        let key = normalize_name(name);
        self.commands.get(&key).or_else(|| {
            self.aliases
                .get(&key)
                .and_then(|canonical| self.commands.get(canonical))
        })
    }

    pub fn iter(&self) -> impl Iterator<Item = &UserCommandMetadata> {
        self.commands.values()
    }

    fn dispatch_error(&self, name: &str) -> Option<String> {
        let key = normalize_name(name);
        self.invalid_commands.get(&key).cloned().or_else(|| {
            self.aliases
                .get(&key)
                .and_then(|canonical| self.invalid_commands.get(canonical))
                .cloned()
        })
    }
}

fn parse_metadata(
    name: String,
    content: &str,
    path: &Path,
) -> (UserCommandMetadata, Vec<LoadError>) {
    let canonical = normalize_name(&name);
    let (metadata, body) = user_commands::parse_frontmatter(content);
    let errors = validate_command_content(&canonical, content, path);
    let mut command = UserCommandMetadata {
        name: canonical,
        body: body.to_string(),
        description: None,
        argument_hint: None,
        allowed_tools: None,
        pausable: false,
        aliases: Vec::new(),
        hidden: false,
    };

    for (key, value) in metadata {
        match key.as_str() {
            "description" => command.description = Some(value),
            "argument-hint" => command.argument_hint = Some(value),
            "allowed-tools" => {
                command.allowed_tools = Some(user_commands::parse_allowed_tools(&value));
            }
            "pausable" => command.pausable = value.trim().eq_ignore_ascii_case("true"),
            "aliases" | "alias" => {
                command.aliases = value
                    .split(',')
                    .map(normalize_name)
                    .filter(|alias| !alias.is_empty())
                    .collect();
            }
            "hidden" => command.hidden = value.trim().eq_ignore_ascii_case("true"),
            _ => {}
        }
    }

    (command, errors)
}

fn validate_command_content(canonical: &str, content: &str, path: &Path) -> Vec<LoadError> {
    let mut errors = Vec::new();
    if canonical.is_empty() {
        errors.push(LoadError {
            path: path.to_path_buf(),
            message: "User command has an empty command name".to_string(),
        });
    }
    if content.trim().is_empty() {
        errors.push(LoadError {
            path: path.to_path_buf(),
            message: format!("User command '/{canonical}' is empty"),
        });
    }

    let Some(first_line_end) = content.find('\n') else {
        return errors;
    };
    let first = content[..first_line_end].trim_end_matches('\r');
    if !is_frontmatter_delimiter(first.trim()) {
        return errors;
    }

    let mut saw_closing = false;
    for raw_line in content[first_line_end + 1..].split_inclusive('\n') {
        let line = raw_line.trim_end_matches(['\r', '\n']);
        let trimmed = line.trim();
        if is_frontmatter_delimiter(trimmed) {
            saw_closing = true;
            break;
        }
        if trimmed.is_empty() {
            continue;
        }
        if let Some((key, _)) = line.split_once(':')
            && !key.trim().is_empty()
        {
            continue;
        }
        errors.push(LoadError {
            path: path.to_path_buf(),
            message: format!(
                "User command '/{canonical}' has invalid frontmatter line {trimmed:?}; expected key: value"
            ),
        });
        break;
    }

    if !saw_closing {
        errors.push(LoadError {
            path: path.to_path_buf(),
            message: format!(
                "User command '/{canonical}' has invalid frontmatter; missing closing --- delimiter"
            ),
        });
    }

    errors
}

fn is_frontmatter_delimiter(value: &str) -> bool {
    value.chars().all(|ch| ch == '-') && value.len() >= 3
}

fn normalize_name(name: &str) -> String {
    name.trim().trim_start_matches('/').to_ascii_lowercase()
}

fn normalize_workspace(workspace: Option<&Path>) -> Option<PathBuf> {
    workspace.map(Path::to_path_buf)
}

fn command_dirs_snapshot(workspace: Option<&Path>) -> Vec<CommandDirSnapshot> {
    user_commands::commands_dirs(workspace)
        .into_iter()
        .map(|path| {
            let modified = std::fs::metadata(&path)
                .and_then(|metadata| metadata.modified())
                .ok();
            let mut files = Vec::new();
            if let Ok(entries) = std::fs::read_dir(&path) {
                for entry in entries.flatten() {
                    let file_path = entry.path();
                    if file_path.extension().and_then(|ext| ext.to_str()) != Some("md") {
                        continue;
                    }
                    let Ok(metadata) = entry.metadata() else {
                        continue;
                    };
                    files.push(CommandFileSnapshot {
                        path: file_path,
                        modified: metadata.modified().ok(),
                        len: metadata.len(),
                    });
                }
            }
            files.sort_by(|a, b| a.path.cmp(&b.path));
            CommandDirSnapshot {
                path,
                modified,
                files,
            }
        })
        .collect()
}

fn registry_lock() -> &'static RwLock<UserCommandRegistryState> {
    USER_COMMAND_REGISTRY.get_or_init(|| RwLock::new(UserCommandRegistryState::default()))
}

fn registry_needs_reload(
    guard: &UserCommandRegistryState,
    workspace: &Option<PathBuf>,
    snapshot: &[CommandDirSnapshot],
) -> bool {
    !guard.initialized || guard.workspace != *workspace || guard.command_dirs_snapshot != snapshot
}

pub fn with_registry_for_workspace<R>(
    workspace: Option<&Path>,
    f: impl FnOnce(&UserCommandRegistry) -> R,
) -> R {
    let workspace = normalize_workspace(workspace);
    let snapshot = command_dirs_snapshot(workspace.as_deref());
    let lock = registry_lock();
    {
        let guard = lock.read().expect("user command registry lock poisoned");
        if !registry_needs_reload(&guard, &workspace, &snapshot) {
            return f(&guard.registry);
        }
    }

    let replacement = UserCommandRegistry::load(workspace.as_deref());
    let mut guard = lock.write().expect("user command registry lock poisoned");
    if registry_needs_reload(&guard, &workspace, &snapshot) {
        guard.initialized = true;
        guard.workspace = workspace;
        guard.command_dirs_snapshot = snapshot;
        guard.registry = replacement;
    }
    f(&guard.registry)
}

pub fn try_dispatch(app: &mut App, input: &str) -> Option<CommandResult> {
    let parts: Vec<&str> = input.trim().splitn(2, ' ').collect();
    let command = normalize_name(parts.first().copied().unwrap_or_default());
    let args = parts.get(1).copied().unwrap_or("").trim();

    let (dispatch_error, metadata) =
        with_registry_for_workspace(Some(&app.workspace), |registry| {
            (
                registry.dispatch_error(&command),
                registry.get(&command).cloned(),
            )
        });
    if let Some(error) = dispatch_error {
        return Some(CommandResult::error(error));
    }

    let metadata = metadata?;

    app.hunt.quarry = None;
    app.hunt.started_at = None;
    app.hunt.verdict = HuntVerdict::Hunting;
    app.hunt.token_budget = None;
    app.hunt.tokens_used = 0;
    app.hunt.time_used_seconds = 0;
    app.hunt.continuation_count = 0;
    app.active_allowed_tools = None;
    app.pausable = false;
    app.paused = false;
    app.paused_quarry = None;
    let mut todos_cleared = false;
    for _ in 0..10 {
        if let Ok(mut todos) = app.todos.try_lock() {
            todos.clear();
            todos_cleared = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    if !todos_cleared {
        tracing::warn!(target: "commands", "todos lock contended or poisoned — previous todos not cleared");
    }

    let mut plan_cleared = false;
    for _ in 0..10 {
        if let Ok(mut plan) = app.plan_state.try_lock() {
            *plan = crate::tools::plan::PlanState::default();
            plan_cleared = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    if !plan_cleared {
        tracing::warn!(target: "commands", "plan_state lock contended or poisoned — previous plan not cleared");
    }

    if let Some(description) = metadata.description.clone() {
        app.hunt.quarry = Some(description);
        app.hunt.started_at = Some(std::time::Instant::now());
    }
    if let Some(tools) = metadata.allowed_tools.clone() {
        app.active_allowed_tools = Some(tools);
    }
    app.pausable = metadata.pausable;

    let message = user_commands::apply_template(&metadata.body, args);
    Some(CommandResult::action(AppAction::SendMessage(message)))
}

#[cfg(test)]
mod tests {}
