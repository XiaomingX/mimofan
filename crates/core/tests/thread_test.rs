use mimofan_core::*;
use serde_json::json;
use std::path::PathBuf;

#[test]
fn truncate_preview_limits_to_120_chars() {
    let long = "a".repeat(200);
    let truncated = truncate_preview(&long);
    assert_eq!(truncated.len(), 120);
}

#[test]
fn truncate_preview_preserves_short_strings() {
    let short = "hello";
    assert_eq!(truncate_preview(short), "hello");
}

#[test]
fn preview_from_initial_history_new() {
    let preview = preview_from_initial_history(&InitialHistory::New);
    assert_eq!(preview, "New conversation");
}

#[test]
fn preview_from_initial_history_forked() {
    let preview = preview_from_initial_history(&InitialHistory::Forked(vec![json!("hello")]));
    assert!(preview.contains("hello"));
}

#[test]
fn preview_from_initial_history_resumed() {
    let preview = preview_from_initial_history(&InitialHistory::Resumed {
        conversation_id: "test".to_string(),
        history: vec![json!("world")],
        rollout_path: PathBuf::from("/tmp/test"),
    });
    assert!(preview.contains("world"));
}
