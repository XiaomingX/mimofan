//! Externalized integration tests for `crates/protocol/src/app.rs`.
//!
//! Originally an inline `#[cfg(test)] mod tests` block — relocated here as a
//! separate integration-test crate without any change to test logic.

use mimofan_protocol::app::*;
use serde_json::json;

#[test]
fn prompt_request_response_format_round_trips() {
    let rf = json!({ "type": "json_object" });
    let req = PromptRequest {
        thread_id: Some("thr_1".to_string()),
        prompt: "hi".to_string(),
        model: Some("mimo-v2.5-pro".to_string()),
        response_format: Some(rf.clone()),
    };
    let value = serde_json::to_value(&req).expect("serialize");
    assert_eq!(value["response_format"], rf);
    let parsed: PromptRequest = serde_json::from_value(value).expect("deserialize");
    assert_eq!(parsed.response_format.as_ref(), Some(&rf));
}

#[test]
fn prompt_request_response_format_omitted_when_none() {
    // `skip_serializing_if = "Option::is_none"` keeps the wire body clean
    // for callers that don't opt in (mirrors StartTurnRequest behavior).
    let req = PromptRequest {
        thread_id: None,
        prompt: "hi".to_string(),
        model: None,
        response_format: None,
    };
    let value = serde_json::to_value(&req).expect("serialize");
    assert!(value.get("response_format").is_none());
}
