use mimofan_protocol::{LocalShellParams, ToolPayload};
use mimofan_tools::*;
use serde_json::json;

#[test]
fn tool_result_success_sets_plain_content() {
    let content = "operation completed successfully";
    let result = ToolResult::success(content);

    assert!(result.success);
    assert_eq!(result.content, content);
    assert!(result.metadata.is_none());
}

#[test]
fn tool_result_json_round_trips_content() {
    let result = ToolResult::json(&json!({"ok": true})).expect("json");
    assert!(result.success);
    assert!(result.content.contains("\"ok\": true"));
}

#[test]
fn helper_extractors_validate_shape() {
    let input = json!({"name": "demo", "count": 7, "enabled": true});
    assert_eq!(required_str(&input, "name").expect("name"), "demo");
    assert_eq!(optional_str(&input, "name"), Some("demo"));
    assert_eq!(optional_str(&input, "missing"), None);
    assert_eq!(optional_str(&input, "count"), None);
    assert_eq!(optional_str(&json!({"name": null}), "name"), None);
    assert_eq!(optional_u64(&input, "count", 0), 7);
    assert!(optional_bool(&input, "enabled", false));
    assert!(matches!(
        required_u64(&input, "name"),
        Err(ToolError::MissingField { .. })
    ));
}

#[test]
fn required_u64_rejects_missing_or_non_integer_values() {
    assert!(matches!(
        required_u64(&json!({}), "count"),
        Err(ToolError::MissingField { .. })
    ));
    assert_eq!(
        required_u64(&json!({"count": 42}), "count").expect("require count field"),
        42
    );
    assert_eq!(
        required_u64(&json!({"count": u64::MAX}), "count").expect("require count field"),
        u64::MAX
    );

    for value in [json!(-1), json!(2.5), json!("42")] {
        assert!(matches!(
            required_u64(&json!({"count": value}), "count"),
            Err(ToolError::MissingField { .. })
        ));
    }
}

#[test]
fn required_str_reports_provided_fields_on_missing_required_field() {
    let input = json!({"path": "src/lib.rs", "content": "new body"});
    let err = required_str(&input, "replace").expect_err("replace is missing");
    let message = err.to_string();
    assert!(message.contains("missing required field 'replace'"));
    assert!(message.contains("Input provided:"));
    assert!(message.contains("path"));
    assert!(message.contains("content"));
}

#[test]
fn tool_error_display_matches_legacy_text() {
    let err = ToolError::missing_field("path");
    assert_eq!(
        err.to_string(),
        "Failed to validate input: missing required field 'path'"
    );
}

#[test]
fn tool_error_missing_field_constructor() {
    let err = ToolError::missing_field("my_field");
    assert!(matches!(err, ToolError::MissingField { field } if field == "my_field"));
}

#[test]
fn tool_error_not_available_displays_reason() {
    let err = ToolError::not_available("custom tool not found");

    assert!(matches!(err, ToolError::NotAvailable { .. }));
    assert_eq!(
        err.to_string(),
        "Failed to locate tool: custom tool not found"
    );
}

#[test]
fn tool_error_permission_denied_displays_reason() {
    let err = ToolError::permission_denied("unauthorized user");

    assert!(matches!(err, ToolError::PermissionDenied { .. }));
    assert_eq!(
        err.to_string(),
        "Failed to authorize tool execution: unauthorized user"
    );
}

#[test]
fn tool_error_execution_failed_displays_reason() {
    let err = ToolError::execution_failed("process crashed");

    assert!(
        matches!(err, ToolError::ExecutionFailed { ref message } if message == "process crashed")
    );
    assert_eq!(err.to_string(), "Failed to execute tool: process crashed");
}

#[test]
fn tool_error_invalid_input_creates_correct_variant() {
    let err = ToolError::invalid_input("test invalid message");
    match err {
        ToolError::InvalidInput { message } => {
            assert_eq!(message, "test invalid message");
        }
        _ => panic!("Expected ToolError::InvalidInput, got {err:?}"),
    }
}

#[test]
fn tool_error_path_escape_display() {
    let path = std::path::PathBuf::from("../outside");
    let err = ToolError::path_escape(path);
    assert_eq!(
        err.to_string(),
        "Failed to resolve path '../outside': path escapes workspace"
    );
}

#[test]
fn tool_call_execution_subject_uses_local_shell_command_and_cwd() {
    let call = ToolCall {
        name: "shell".to_string(),
        payload: ToolPayload::LocalShell {
            params: LocalShellParams {
                command: "ls -l".to_string(),
                cwd: Some("/custom/dir".to_string()),
                timeout_ms: None,
            },
        },
        source: ToolCallSource::Direct,
        raw_tool_call_id: None,
    };

    assert_eq!(
        call.execution_subject("/fallback/dir"),
        ("ls -l".to_string(), "/custom/dir".to_string(), "shell")
    );
}

#[test]
fn tool_call_execution_subject_falls_back_for_shell_without_cwd() {
    let call = ToolCall {
        name: "shell".to_string(),
        payload: ToolPayload::LocalShell {
            params: LocalShellParams {
                command: "echo hello".to_string(),
                cwd: None,
                timeout_ms: None,
            },
        },
        source: ToolCallSource::Direct,
        raw_tool_call_id: None,
    };

    assert_eq!(
        call.execution_subject("/fallback/dir"),
        (
            "echo hello".to_string(),
            "/fallback/dir".to_string(),
            "shell"
        )
    );
}

#[test]
fn tool_call_execution_subject_uses_tool_name_for_non_shell_payloads() {
    let call = ToolCall {
        name: "my_tool".to_string(),
        payload: ToolPayload::Function {
            arguments: "{}".to_string(),
        },
        source: ToolCallSource::Direct,
        raw_tool_call_id: None,
    };

    assert_eq!(
        call.execution_subject("/fallback/dir"),
        ("my_tool".to_string(), "/fallback/dir".to_string(), "tool")
    );
}

#[test]
fn replace_file_content_str_replaces_target_within_line_range() {
    let text = "line 1\nline 2\nline 3\nline 4\n";
    let res = replace_file_content_str(text, 2, 3, "line 2", "line TWO")
        .expect("replace file content within line range");
    assert_eq!(res, "line 1\nline TWO\nline 3\nline 4\n");
}

#[test]
fn replace_file_content_str_fails_when_target_not_in_range() {
    let text = "line 1\nline 2\nline 3\nline 4\n";
    let err = replace_file_content_str(text, 1, 2, "line 4", "line FOUR").unwrap_err();
    assert!(err.to_string().contains("target content not found"));
}
