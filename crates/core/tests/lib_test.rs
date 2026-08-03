use mimofan_core::*;
use mimofan_state::{SessionSource, StateStore, ThreadMetadata, ThreadStatus as PersistedThreadStatus};
use mimofan_tools::{ToolCall, ToolCallSource};
use mimofan_protocol::{EventFrame, LocalShellParams, ToolPayload};
use mimofan_execpolicy::ExecApprovalRequirement;
use serde_json::json;
use std::path::PathBuf;
use uuid::Uuid;

fn temp_core_state(name: &str) -> StateStore {
    let dir =
        std::env::temp_dir().join(format!("mimofan-core-{name}-{}", Uuid::new_v4().simple()));
    std::fs::create_dir_all(&dir).expect("create temp state dir");
    StateStore::open(Some(dir.join("state.db"))).expect("open state store")
}

fn test_thread_metadata(id: &str) -> ThreadMetadata {
    ThreadMetadata {
        id: id.to_string(),
        rollout_path: None,
        preview: "test thread".to_string(),
        ephemeral: false,
        model_provider: "deepseek".to_string(),
        created_at: 10,
        updated_at: 10,
        status: PersistedThreadStatus::Running,
        path: None,
        cwd: PathBuf::from("/tmp/mimo"),
        cli_version: "0.0.0-test".to_string(),
        source: SessionSource::Interactive,
        name: None,
        sandbox_policy: None,
        approval_mode: None,
        archived: false,
        archived_at: None,
        git_sha: None,
        git_branch: None,
        git_origin_url: None,
        memory_mode: None,
        current_leaf_id: None,
    }
}

#[test]
fn permission_path_for_call_extracts_function_path_argument() {
    let call = ToolCall {
        name: "read_file".to_string(),
        payload: ToolPayload::Function {
            arguments: json!({ "path": "README.md" }).to_string(),
        },
        source: ToolCallSource::Direct,
        raw_tool_call_id: None,
    };

    assert_eq!(
        permission_path_for_call(&call).as_deref(),
        Some("README.md")
    );
}

#[test]
fn permission_path_for_call_extracts_mcp_path_argument() {
    let call = ToolCall {
        name: "mcp_fs_read".to_string(),
        payload: ToolPayload::Mcp {
            server: "fs".to_string(),
            tool: "read".to_string(),
            raw_arguments: json!({ "path": "secrets/token.txt" }),
            raw_tool_call_id: None,
        },
        source: ToolCallSource::Direct,
        raw_tool_call_id: None,
    };

    assert_eq!(
        permission_path_for_call(&call).as_deref(),
        Some("secrets/token.txt")
    );
}

#[test]
fn permission_path_for_call_ignores_shell_payload() {
    let call = ToolCall {
        name: "exec_shell".to_string(),
        payload: ToolPayload::LocalShell {
            params: LocalShellParams {
                command: "cargo test".to_string(),
                cwd: None,
                timeout_ms: None,
            },
        },
        source: ToolCallSource::Direct,
        raw_tool_call_id: None,
    };

    assert_eq!(permission_path_for_call(&call), None);
}

#[test]
fn thread_goal_progress_accumulates_durable_accounting() {
    let store = temp_core_state("thread-goal-progress");
    store
        .upsert_thread(&test_thread_metadata("thread-1"))
        .expect("upsert thread");
    let mut manager = ThreadManager::new(store);
    manager
        .set_thread_goal(&ThreadGoalSetParams {
            thread_id: "thread-1".to_string(),
            objective: "Carry the goal across turns".to_string(),
            token_budget: Some(2_000),
        })
        .expect("set goal")
        .expect("goal exists");

    let updated = manager
        .record_thread_goal_progress(&ThreadGoalProgressParams {
            thread_id: "thread-1".to_string(),
            token_delta: 750,
            time_delta_seconds: 12,
            record_continuation: true,
        })
        .expect("record progress")
        .expect("goal exists");

    assert_eq!(updated.tokens_used, 750);
    assert_eq!(updated.time_used_seconds, 12);
    assert_eq!(updated.continuation_count, 1);

    let persisted = manager
        .get_thread_goal(&ThreadGoalGetParams {
            thread_id: "thread-1".to_string(),
        })
        .expect("read goal")
        .expect("goal exists");
    assert_eq!(persisted.tokens_used, 750);
    assert_eq!(persisted.time_used_seconds, 12);
    assert_eq!(persisted.continuation_count, 1);
}

#[test]
fn approval_request_frame_includes_matched_rule() {
    let requirement = ExecApprovalRequirement::NeedsApproval {
        reason: "Typed ask rule 'tool=exec_shell command=cargo test' requires approval."
            .to_string(),
        proposed_execpolicy_amendment: None,
        proposed_network_policy_amendments: Vec::new(),
    };

    let frame = approval_request_frame(
        &requirement,
        Some("tool=exec_shell command=cargo test"),
        "call-1".to_string(),
        "approval-1".to_string(),
        "turn-1".to_string(),
        "cargo test --workspace".to_string(),
        "/repo".to_string(),
    )
    .expect("approval frame");

    let EventFrame::ExecApprovalRequest { request } = frame else {
        panic!("expected exec approval request frame");
    };
    assert_eq!(
        request.matched_rule.as_deref(),
        Some("tool=exec_shell command=cargo test")
    );
    assert_eq!(request.reason, requirement.reason());
}

#[test]
fn user_input_request_frame_lifts_questions_from_arguments() {
    let arguments = r#"{"questions":[{"header":"Scope","id":"scope","question":"Which?","options":[{"label":"A","description":"a"},{"label":"B","description":"b"}],"allow_free_text":true}]}"#;
    let frame = user_input_request_frame(
        "call-1".to_string(),
        "turn-1".to_string(),
        "ui-1".to_string(),
        arguments,
    )
    .expect("user input frame");

    let EventFrame::UserInputRequest { request } = frame else {
        panic!("expected user_input_request frame");
    };
    assert_eq!(request.call_id, "call-1");
    assert_eq!(request.turn_id, "turn-1");
    assert_eq!(request.request_id, "ui-1");
    assert_eq!(request.questions.len(), 1);
    assert_eq!(request.questions[0].id, "scope");
    assert!(request.questions[0].allow_free_text);
    assert!(!request.questions[0].multi_select);
    assert_eq!(request.questions[0].options.len(), 2);
}

#[test]
fn user_input_request_frame_returns_none_on_invalid_arguments() {
    let frame = user_input_request_frame(
        "call-1".to_string(),
        "turn-1".to_string(),
        "ui-1".to_string(),
        "not json",
    );
    assert!(frame.is_none());

    let frame = user_input_request_frame(
        "call-1".to_string(),
        "turn-1".to_string(),
        "ui-1".to_string(),
        r#"{"foo":"bar"}"#,
    );
    assert!(frame.is_none());
}
