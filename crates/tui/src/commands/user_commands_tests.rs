// Tests relocated from src/commands/user_commands.rs

use crate::config::Config;
use crate::tui::app::{App, AppMode, TuiOptions};
use tempfile::TempDir;

fn default_tui_options(tmp: &TempDir) -> TuiOptions {
    TuiOptions {
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
        memory_dir: tmp.path().join("memory"),
        notes_path: tmp.path().join("notes.txt"),
        mcp_config_path: tmp.path().join("mcp.json"),
        use_memory: false,
        start_in_agent_mode: false,
        skip_onboarding: true,
        yolo: false,
        resume_session_id: None,
        initial_input: None,
    }
}

#[test]
fn test_mode_shortcut_commands() {
    let tmp = TempDir::new().expect("create temp dir");
    let mut app = App::new(default_tui_options(&tmp), &Config::default());

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
    let tmp = TempDir::new().expect("create temp dir");
    let mut app = App::new(default_tui_options(&tmp), &Config::default());

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
    let msg = res.message.expect("command result message");
    assert!(msg.contains("No snapshots yet"));
    assert!(msg.contains("💡 Tip"));
}

#[test]
fn test_grill_me_command() {
    let tmp = TempDir::new().expect("create temp dir");
    let mut app = App::new(default_tui_options(&tmp), &Config::default());

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
    let tmp = TempDir::new().expect("create temp dir");
    let mut app = App::new(default_tui_options(&tmp), &Config::default());

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
    let tmp = TempDir::new().expect("create temp dir");
    let mut app = App::new(default_tui_options(&tmp), &Config::default());

    // /skill-run without args returns error
    let res = crate::commands::execute("/skill-run", &mut app);
    assert!(res.is_error);

    // create a dummy markdown file
    let dummy_path = tmp.path().join("test_skill.md");
    std::fs::write(&dummy_path, "# Test Skill\nDescription").expect("write temp file");

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
    let tmp = TempDir::new().expect("create temp dir");
    let mut app = App::new(default_tui_options(&tmp), &Config::default());

    // /plugin-test without args returns error
    let res = crate::commands::execute("/plugin-test", &mut app);
    assert!(res.is_error);

    // create a dummy plugin script
    let dummy_script = tmp.path().join("test_plugin.sh");
    std::fs::write(
        &dummy_script,
        "#!/bin/sh\n# name: test-tool\n# description: test\n# schema: {}\n\necho '{\"content\":\"success\",\"success\":true}'",
    )
    .expect("unexpected None/Err in test");

    // Make executable on unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&dummy_script)
            .expect("read file metadata")
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&dummy_script, perms).expect("set file permissions");
    }

    let cmd = format!("/plugin-test {} {}", dummy_script.display(), "{}");
    let res = crate::commands::execute(&cmd, &mut app);
    assert!(!res.is_error);
    let msg = res.message.expect("command result message");
    assert!(msg.contains("test-tool"));
    assert!(msg.contains("success"));
}

#[test]
fn test_tools_inspect_command() {
    let tmp = TempDir::new().expect("create temp dir");
    let mut app = App::new(default_tui_options(&tmp), &Config::default());

    // /tools list
    let res = crate::commands::execute("/tools", &mut app);
    assert!(!res.is_error);
    let msg = res.message.expect("command result message");
    assert!(msg.contains("Available Native and Plugin Tools"));
    assert!(msg.contains("read_file"));

    // /tools read_file (inspect a single tool details)
    let res = crate::commands::execute("/tools read_file", &mut app);
    assert!(!res.is_error);
    let msg = res.message.expect("command result message");
    assert!(msg.contains("Tool: read_file"));
    assert!(msg.contains("Input Schema"));
}

#[test]
fn test_code_review_alias_and_schema() {
    let tmp = TempDir::new().expect("create temp dir");
    let mut app = App::new(default_tui_options(&tmp), &Config::default());

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
    let tmp = TempDir::new().expect("create temp dir");
    let mut app = App::new(default_tui_options(&tmp), &Config::default());

    // 1. /make-plan generates a message to Plan mode
    let res = crate::commands::execute("/make-plan Migrate configuration", &mut app);
    assert!(!res.is_error);
    assert_eq!(app.mode, AppMode::Plan);
    let msg = res.message.expect("command result message");
    assert!(msg.contains("Switched to Plan mode"));
    let action = res.action.expect("command result action");
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
    let action = res.action.expect("command result action");
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
    let action = res.action.expect("command result action");
    match action {
        crate::tui::app::AppAction::SendMessage(prompt) => {
            assert!(prompt.contains("execute all remaining pending checklist steps"));
        }
        _ => panic!("Expected SendMessage action"),
    }
}

#[test]
fn test_exit_plan_from_plan_mode() {
    let tmp = TempDir::new().expect("create temp dir");
    let mut app = App::new(default_tui_options(&tmp), &Config::default());

    // Enter Plan mode first.
    let res = crate::commands::execute("/plan", &mut app);
    assert!(!res.is_error);
    assert_eq!(app.mode, AppMode::Plan);

    // /exit_plan (and aliases) should emit a ModeChanged(Agent) action.
    // The actual mode switch is applied by the caller that processes the
    // action, so we verify the action rather than `app.mode` here.
    for cmd in ["/exit_plan", "/leave_plan", "/tuichu_plan"] {
        let res = crate::commands::execute(cmd, &mut app);
        assert!(!res.is_error, "command failed: {cmd}");
        let action = res.action.expect("command result action");
        match action {
            crate::tui::app::AppAction::ModeChanged(mode) => {
                assert_eq!(mode, AppMode::Agent, "wrong target mode from {cmd}");
            }
            _ => panic!("Expected ModeChanged action from {cmd}"),
        }
    }
}

#[test]
fn test_exit_plan_outside_plan_mode_is_noop() {
    let tmp = TempDir::new().expect("create temp dir");
    let mut app = App::new(default_tui_options(&tmp), &Config::default());

    // Already in Agent mode.
    assert_eq!(app.mode, AppMode::Agent);

    let res = crate::commands::execute("/exit_plan", &mut app);
    // No mode change action, no error — just an informational message.
    assert!(!res.is_error);
    assert!(res.action.is_none(), "should not emit a mode-change action");
    let msg = res.message.expect("command result message");
    assert!(msg.contains("不在 Plan 模式"));
    assert_eq!(app.mode, AppMode::Agent);
}

#[test]
fn test_rewind_shows_diff_preview_and_reverts_conversation() {
    use crate::snapshot::SnapshotRepo;

    let tmp = TempDir::new().expect("create temp dir");
    let mut app = App::new(default_tui_options(&tmp), &Config::default());

    // Seed two user messages so conversation_len tracking is meaningful.
    app.api_messages.push(crate::models::Message {
        role: "user".to_string(),
        content: vec![crate::models::ContentBlock::Text {
            text: "first".to_string(),
            cache_control: None,
        }],
    });
    app.api_messages.push(crate::models::Message {
        role: "user".to_string(),
        content: vec![crate::models::ContentBlock::Text {
            text: "second".to_string(),
            cache_control: None,
        }],
    });
    let conv_len_before = app.api_messages.len();

    // Create a workspace file and snapshot it at the current conversation length.
    std::fs::write(tmp.path().join("file.txt"), "v1").unwrap();
    let repo = SnapshotRepo::open_or_init(&app.workspace).unwrap();
    repo.snapshot("snapshot-at-conv", conv_len_before).unwrap();

    // Mutate the file so a rollback has something to revert.
    std::fs::write(tmp.path().join("file.txt"), "v2-changed").unwrap();

    // Enter trusted mode so /rewind may mutate files.
    app.trust_mode = true;

    // /rewind 1 should show a diff preview and roll back BOTH files and chat.
    let res = crate::commands::execute("/rewind 1", &mut app);
    assert!(!res.is_error, "rewind should succeed: {:?}", res.message);
    let msg = res.message.expect("command result message");
    assert!(
        msg.contains("Will change"),
        "expected diff preview, got: {msg}"
    );
    assert!(
        msg.contains("conversation history reverted"),
        "expected combined revert, got: {msg}"
    );

    // File reverted.
    let restored = std::fs::read_to_string(tmp.path().join("file.txt")).unwrap();
    assert_eq!(restored, "v1");

    // Conversation truncated to the snapshot's recorded length (<= before).
    assert!(
        app.api_messages.len() <= conv_len_before,
        "conversation should be truncated, got {} (was {conv_len_before})",
        app.api_messages.len()
    );
}

#[test]
fn test_rewind_cross_session_can_read_prior_snapshot() {
    use crate::snapshot::SnapshotRepo;

    let tmp = TempDir::new().expect("create temp dir");
    let app = App::new(default_tui_options(&tmp), &Config::default());

    // Simulate a prior session: a file + snapshot on the same workspace.
    std::fs::write(tmp.path().join("prior.txt"), "original").unwrap();
    let repo = SnapshotRepo::open_or_init(&app.workspace).unwrap();
    repo.snapshot("prior-session", 0).unwrap();

    // A "resumed" session on the SAME workspace must see the prior snapshot
    // (CodeBuddy exposes checkpoints across resumed sessions).
    let resumed = SnapshotRepo::open_or_init(&app.workspace).unwrap();
    let snapshots = resumed.list(10).unwrap();
    assert!(
        snapshots.iter().any(|s| s.label == "prior-session"),
        "resumed session could not read prior snapshot"
    );
}
