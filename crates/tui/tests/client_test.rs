// Tests relocated from src/client.rs (issue #547 Phase 3).

use mimofan::client::*;
use mimofan::models::MessageRequest;
use serde_json::json;

use mimofan::config::ApiProvider;

#[test]
fn anthropic_compatible_provider_uses_messages_protocol() {
    // Anthropic-compatible mode always dispatches to the Anthropic Messages client.
    assert!(api_provider_uses_anthropic_messages(
        ApiProvider::AnthropicCompatible,
        "https://api.anthropic.com/v1"
    ));
}

#[test]
fn openai_compatible_provider_uses_chat_completions() {
    // OpenAI-compatible mode never dispatches to the Anthropic Messages client,
    // regardless of the configured base URL.
    assert!(!api_provider_uses_anthropic_messages(
        ApiProvider::OpenAiCompatible,
        "https://api.anthropic.com/v1"
    ));
}

#[test]
fn gemini_compatible_provider_uses_chat_completions() {
    assert!(!api_provider_uses_anthropic_messages(
        ApiProvider::GeminiCompatible,
        "https://generativelanguage.googleapis.com/v1beta"
    ));
}

// ── MessageRequest::response_format round-trip (#0.0.3-rc.3) ──────────
// The OpenAI Chat Completions body builder forwards `response_format`
// (e.g. XiaomiMiMo `{"type":"json_object"}` JSON mode). Lock in the
// field's serde shape so downstream `serde_json::from_value` of a
// caller-supplied body still parses.
#[test]
fn message_request_response_format_round_trips() {
    let rf = json!({ "type": "json_object" });
    let req = MessageRequest {
        model: "mimo-v2.5-pro".to_string(),
        messages: vec![],
        max_tokens: 256,
        system: None,
        tools: None,
        tool_choice: None,
        metadata: None,
        thinking: None,
        reasoning_effort: None,
        stream: None,
        temperature: None,
        top_p: None,
        response_format: Some(rf.clone()),
    };
    let value = serde_json::to_value(&req).expect("serialize");
    assert_eq!(value["response_format"], rf);
    let parsed: MessageRequest = serde_json::from_value(value).expect("deserialize");
    assert_eq!(parsed.response_format.as_ref(), Some(&rf));
}

#[test]
fn message_request_response_format_omitted_when_none() {
    // `skip_serializing_if = "Option::is_none"` keeps the wire body
    // clean for callers that don't opt in.
    let req = MessageRequest {
        model: "mimo-v2.5-pro".to_string(),
        messages: vec![],
        max_tokens: 256,
        system: None,
        tools: None,
        tool_choice: None,
        metadata: None,
        thinking: None,
        reasoning_effort: None,
        stream: None,
        temperature: None,
        top_p: None,
        response_format: None,
    };
    let value = serde_json::to_value(&req).expect("serialize");
    assert!(value.get("response_format").is_none());
}
