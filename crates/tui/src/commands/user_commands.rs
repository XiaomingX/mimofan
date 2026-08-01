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
//! 2. `<workspace>/.claude/commands/`    (Claude Code interop)
//! 4. `<workspace>/.cursor/commands/`    (Cursor interop)
//! 5. `~/.mimofan/commands/`           (user-global)
//! 6. `~/.mimofan/commands/`            (legacy user-global)
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

/// Return all candidate commands directories in precedence order.
pub(crate) fn commands_dirs(workspace: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(ws) = workspace {
        dirs.push(ws.join(".mimofan").join("commands"));
        dirs.push(ws.join(".claude").join("commands"));
        dirs.push(ws.join(".cursor").join("commands"));
    }
    dirs.push(global_commands_dir());
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
mod tests {
    use crate::config::Config;
    use crate::tui::app::{App, AppMode, TuiOptions};
    use tempfile::TempDir;

    #[test]
    fn test_mode_shortcut_commands() {
        let tmp = TempDir::new().unwrap();
        let options = TuiOptions {
            model: "deepseek-v4-pro".to_string(),
            workspace: tmp.path().to_path_buf(),
            config_path: None,
            config_profile: None,
            allow_shell: false,
            use_alt_screen: true,
            use_mouse_capture: false,
            use_bracketed_paste: true,
            max_subagents: 1,
            skills_dir: tmp.path().join("skills"),
            memory_path: tmp.path().join("memory.md"),
            notes_path: tmp.path().join("notes.txt"),
            mcp_config_path: tmp.path().join("mcp.json"),
            use_memory: false,
            start_in_agent_mode: false,
            skip_onboarding: true,
            yolo: false,
            resume_session_id: None,
            initial_input: None,
        };
        let mut app = App::new(options, &Config::default());

        // Default mode
        assert_eq!(app.mode, AppMode::Agent);

        // Test /plan command
        let res = crate::commands::execute("/plan", &mut app);
        assert!(!res.is_error);
        assert_eq!(app.mode, AppMode::Plan);

        // Test /auto command
        let res = crate::commands::execute("/auto", &mut app);
        assert!(!res.is_error);
        assert_eq!(app.mode, AppMode::Agent);

        // Test /yolo command
        let res = crate::commands::execute("/yolo", &mut app);
        assert!(!res.is_error);
        assert_eq!(app.mode, AppMode::Yolo);
    }

    #[test]
    fn test_rewind_command() {
        let tmp = TempDir::new().unwrap();
        let options = TuiOptions {
            model: "deepseek-v4-pro".to_string(),
            workspace: tmp.path().to_path_buf(),
            config_path: None,
            config_profile: None,
            allow_shell: false,
            use_alt_screen: true,
            use_mouse_capture: false,
            use_bracketed_paste: true,
            max_subagents: 1,
            skills_dir: tmp.path().join("skills"),
            memory_path: tmp.path().join("memory.md"),
            notes_path: tmp.path().join("notes.txt"),
            mcp_config_path: tmp.path().join("mcp.json"),
            use_memory: false,
            start_in_agent_mode: false,
            skip_onboarding: true,
            yolo: false,
            resume_session_id: None,
            initial_input: None,
        };
        let mut app = App::new(options, &Config::default());

        // Test /rewind chat action
        let res = crate::commands::execute("/rewind chat", &mut app);
        assert!(!res.is_error);
        assert_eq!(
            res.action,
            Some(crate::tui::app::AppAction::OpenBacktrackOverlay)
        );

        // Test /rewind (no snapshots) should return the "No snapshots yet" message with tip
        let res = crate::commands::execute("/rewind", &mut app);
        assert!(!res.is_error);
        let msg = res.message.unwrap();
        assert!(msg.contains("No snapshots yet"));
        assert!(msg.contains("💡 Tip"));
    }

    #[test]
    fn test_grill_me_command() {
        let tmp = TempDir::new().unwrap();
        let options = TuiOptions {
            model: "deepseek-v4-pro".to_string(),
            workspace: tmp.path().to_path_buf(),
            config_path: None,
            config_profile: None,
            allow_shell: false,
            use_alt_screen: true,
            use_mouse_capture: false,
            use_bracketed_paste: true,
            max_subagents: 1,
            skills_dir: tmp.path().join("skills"),
            memory_path: tmp.path().join("memory.md"),
            notes_path: tmp.path().join("notes.txt"),
            mcp_config_path: tmp.path().join("mcp.json"),
            use_memory: false,
            start_in_agent_mode: false,
            skip_onboarding: true,
            yolo: false,
            resume_session_id: None,
            initial_input: None,
        };
        let mut app = App::new(options, &Config::default());

        // /grill-me without arg returns error
        let res = crate::commands::execute("/grill-me", &mut app);
        assert!(res.is_error);

        // /grill-me with arg starts clarification flow
        let res = crate::commands::execute("/grill-me implement feature X", &mut app);
        assert!(!res.is_error);
        assert!(app.active_skill.is_some());
        assert_eq!(
            res.action,
            Some(crate::tui::app::AppAction::SendMessage(
                "implement feature X".to_string()
            ))
        );
    }

    #[test]
    fn test_simplify_command() {
        let tmp = TempDir::new().unwrap();
        let options = TuiOptions {
            model: "deepseek-v4-pro".to_string(),
            workspace: tmp.path().to_path_buf(),
            config_path: None,
            config_profile: None,
            allow_shell: false,
            use_alt_screen: true,
            use_mouse_capture: false,
            use_bracketed_paste: true,
            max_subagents: 1,
            skills_dir: tmp.path().join("skills"),
            memory_path: tmp.path().join("memory.md"),
            notes_path: tmp.path().join("notes.txt"),
            mcp_config_path: tmp.path().join("mcp.json"),
            use_memory: false,
            start_in_agent_mode: false,
            skip_onboarding: true,
            yolo: false,
            resume_session_id: None,
            initial_input: None,
        };
        let mut app = App::new(options, &Config::default());

        // /simplify without arg returns error
        let res = crate::commands::execute("/simplify", &mut app);
        assert!(res.is_error);

        // /simplify with arg starts simplification
        let res = crate::commands::execute("/simplify crates/tui/src/tui/ui.rs", &mut app);
        assert!(!res.is_error);
        assert!(app.active_skill.is_some());
        assert_eq!(
            res.action,
            Some(crate::tui::app::AppAction::SendMessage(
                "crates/tui/src/tui/ui.rs".to_string()
            ))
        );
    }

    #[test]
    fn test_skill_run_command() {
        let tmp = TempDir::new().unwrap();
        let options = TuiOptions {
            model: "deepseek-v4-pro".to_string(),
            workspace: tmp.path().to_path_buf(),
            config_path: None,
            config_profile: None,
            allow_shell: false,
            use_alt_screen: true,
            use_mouse_capture: false,
            use_bracketed_paste: true,
            max_subagents: 1,
            skills_dir: tmp.path().join("skills"),
            memory_path: tmp.path().join("memory.md"),
            notes_path: tmp.path().join("notes.txt"),
            mcp_config_path: tmp.path().join("mcp.json"),
            use_memory: false,
            start_in_agent_mode: false,
            skip_onboarding: true,
            yolo: false,
            resume_session_id: None,
            initial_input: None,
        };
        let mut app = App::new(options, &Config::default());

        // /skill-run without args returns error
        let res = crate::commands::execute("/skill-run", &mut app);
        assert!(res.is_error);

        // create a dummy markdown file
        let dummy_path = tmp.path().join("test_skill.md");
        std::fs::write(&dummy_path, "# Test Skill\nDescription").unwrap();

        // /skill-run with valid file path starts skill activation
        let cmd = format!("/skill-run {} arg1 arg2", dummy_path.display());
        let res = crate::commands::execute(&cmd, &mut app);
        assert!(!res.is_error);
        assert!(app.active_skill.is_some());
        assert_eq!(
            res.action,
            Some(crate::tui::app::AppAction::SendMessage(
                "arg1 arg2".to_string()
            ))
        );
    }

    #[test]
    fn test_plugin_test_command() {
        let tmp = TempDir::new().unwrap();
        let options = TuiOptions {
            model: "deepseek-v4-pro".to_string(),
            workspace: tmp.path().to_path_buf(),
            config_path: None,
            config_profile: None,
            allow_shell: false,
            use_alt_screen: true,
            use_mouse_capture: false,
            use_bracketed_paste: true,
            max_subagents: 1,
            skills_dir: tmp.path().join("skills"),
            memory_path: tmp.path().join("memory.md"),
            notes_path: tmp.path().join("notes.txt"),
            mcp_config_path: tmp.path().join("mcp.json"),
            use_memory: false,
            start_in_agent_mode: false,
            skip_onboarding: true,
            yolo: false,
            resume_session_id: None,
            initial_input: None,
        };
        let mut app = App::new(options, &Config::default());

        // /plugin-test without args returns error
        let res = crate::commands::execute("/plugin-test", &mut app);
        assert!(res.is_error);

        // create a dummy plugin script
        let dummy_script = tmp.path().join("test_plugin.sh");
        std::fs::write(
            &dummy_script,
            "#!/bin/sh\n# name: test-tool\n# description: test\n# schema: {}\n\necho '{\"content\":\"success\",\"success\":true}'",
        )
        .unwrap();

        // Make executable on unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&dummy_script).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&dummy_script, perms).unwrap();
        }

        let cmd = format!("/plugin-test {} {}", dummy_script.display(), "{}");
        let res = crate::commands::execute(&cmd, &mut app);
        assert!(!res.is_error);
        let msg = res.message.unwrap();
        assert!(msg.contains("test-tool"));
        assert!(msg.contains("success"));
    }

    #[test]
    fn test_tools_inspect_command() {
        let tmp = TempDir::new().unwrap();
        let options = TuiOptions {
            model: "deepseek-v4-pro".to_string(),
            workspace: tmp.path().to_path_buf(),
            config_path: None,
            config_profile: None,
            allow_shell: false,
            use_alt_screen: true,
            use_mouse_capture: false,
            use_bracketed_paste: true,
            max_subagents: 1,
            skills_dir: tmp.path().join("skills"),
            memory_path: tmp.path().join("memory.md"),
            notes_path: tmp.path().join("notes.txt"),
            mcp_config_path: tmp.path().join("mcp.json"),
            use_memory: false,
            start_in_agent_mode: false,
            skip_onboarding: true,
            yolo: false,
            resume_session_id: None,
            initial_input: None,
        };
        let mut app = App::new(options, &Config::default());

        // /tools list
        let res = crate::commands::execute("/tools", &mut app);
        assert!(!res.is_error);
        let msg = res.message.unwrap();
        assert!(msg.contains("Available Native and Plugin Tools"));
        assert!(msg.contains("read_file"));

        // /tools read_file (inspect a single tool details)
        let res = crate::commands::execute("/tools read_file", &mut app);
        assert!(!res.is_error);
        let msg = res.message.unwrap();
        assert!(msg.contains("Tool: read_file"));
        assert!(msg.contains("Input Schema"));
    }

    #[test]
    fn test_code_review_alias_and_schema() {
        let tmp = TempDir::new().unwrap();
        let options = TuiOptions {
            model: "deepseek-v4-pro".to_string(),
            workspace: tmp.path().to_path_buf(),
            config_path: None,
            config_profile: None,
            allow_shell: false,
            use_alt_screen: true,
            use_mouse_capture: false,
            use_bracketed_paste: true,
            max_subagents: 1,
            skills_dir: tmp.path().join("skills"),
            memory_path: tmp.path().join("memory.md"),
            notes_path: tmp.path().join("notes.txt"),
            mcp_config_path: tmp.path().join("mcp.json"),
            use_memory: false,
            start_in_agent_mode: false,
            skip_onboarding: true,
            yolo: false,
            resume_session_id: None,
            initial_input: None,
        };
        let mut app = App::new(options, &Config::default());

        // Test /code-review without arg returns error (behaves like /review target)
        let res = crate::commands::execute("/code-review", &mut app);
        assert!(res.is_error);

        // Test /code-review with arg activates review skill
        let res = crate::commands::execute("/code-review crates/tui/src/tui/app.rs", &mut app);
        assert!(!res.is_error);
        assert!(app.active_skill.is_some());
        assert_eq!(
            res.action,
            Some(crate::tui::app::AppAction::SendMessage(
                "crates/tui/src/tui/app.rs".to_string()
            ))
        );

        // Test parsing code reviewer output with security_issues
        let raw_json = r#"{
            "summary": "overall secure",
            "issues": [],
            "security_issues": [
                {
                    "severity": "error",
                    "category": "Secret Leakage",
                    "title": "Hardcoded secret",
                    "description": "access_token hardcoded in source",
                    "path": "src/oauth.rs",
                    "line": 42
                }
            ],
            "suggestions": [],
            "overall_assessment": "approved with warning"
        }"#;

        let output = crate::tools::review::ReviewOutput::from_str(raw_json);
        assert_eq!(output.summary, "overall secure");
        assert_eq!(output.security_issues.len(), 1);
        assert_eq!(output.security_issues[0].severity, "error");
        assert_eq!(output.security_issues[0].title, "Hardcoded secret");
        assert_eq!(
            output.security_issues[0].path,
            Some("src/oauth.rs".to_string())
        );
        assert_eq!(output.security_issues[0].line, Some(42));
    }

    #[test]
    fn test_make_plan_and_do_commands() {
        use crate::tools::todo::TodoStatus;
        let tmp = TempDir::new().unwrap();
        let options = TuiOptions {
            model: "deepseek-v4-pro".to_string(),
            workspace: tmp.path().to_path_buf(),
            config_path: None,
            config_profile: None,
            allow_shell: false,
            use_alt_screen: true,
            use_mouse_capture: false,
            use_bracketed_paste: true,
            max_subagents: 1,
            skills_dir: tmp.path().join("skills"),
            memory_path: tmp.path().join("memory.md"),
            notes_path: tmp.path().join("notes.txt"),
            mcp_config_path: tmp.path().join("mcp.json"),
            use_memory: false,
            start_in_agent_mode: false,
            skip_onboarding: true,
            yolo: false,
            resume_session_id: None,
            initial_input: None,
        };
        let mut app = App::new(options, &Config::default());

        // 1. /make-plan generates a message to Plan mode
        let res = crate::commands::execute("/make-plan Migrate configuration", &mut app);
        assert!(!res.is_error);
        assert_eq!(app.mode, AppMode::Plan);
        let msg = res.message.unwrap();
        assert!(msg.contains("Switched to Plan mode"));
        let action = res.action.unwrap();
        match action {
            crate::tui::app::AppAction::SendMessage(prompt) => {
                assert!(prompt.contains("Task: Migrate configuration"));
            }
            _ => panic!("Expected SendMessage action"),
        }

        // 2. /do fails if no checklist steps found
        let res = crate::commands::execute("/do", &mut app);
        assert!(res.is_error);

        // 3. Add dummy todo items
        if let Ok(mut todos) = app.todos.try_lock() {
            todos.add("Step one".to_string(), TodoStatus::Pending);
            todos.add("Step two".to_string(), TodoStatus::Pending);
        }

        // 4. /do next switches to Agent mode and starts first step
        let res = crate::commands::execute("/do", &mut app);
        assert!(!res.is_error);
        assert_eq!(app.mode, AppMode::Agent);
        let action = res.action.unwrap();
        match action {
            crate::tui::app::AppAction::SendMessage(prompt) => {
                assert!(prompt.contains("Step 1: Step one"));
            }
            _ => panic!("Expected SendMessage action"),
        }

        // Check first step was marked InProgress
        if let Ok(todos) = app.todos.try_lock() {
            let snap = todos.snapshot();
            assert_eq!(snap.items[0].status, TodoStatus::InProgress);
        }

        // 5. /do all executes all pending steps
        let res = crate::commands::execute("/do all", &mut app);
        assert!(!res.is_error);
        let action = res.action.unwrap();
        match action {
            crate::tui::app::AppAction::SendMessage(prompt) => {
                assert!(prompt.contains("execute all remaining pending checklist steps"));
            }
            _ => panic!("Expected SendMessage action"),
        }
    }
}
