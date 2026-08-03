//! External tests for the anthropic client module.
//!
//! These tests were migrated from inline `#[cfg(test)] mod tests` in
//! `crates/tui/src/client/anthropic.rs` to improve compilation parallelism
//! and keep the source file focused on implementation.

use mimofan::client::anthropic::{
    XIAOMIMIMO_LIVE_RESPONSE, anthropic_messages_url, anthropic_model_rejects_sampling,
    anthropic_tool_choice, apply_anthropic_cache_breakpoints, parse_anthropic_usage,
};
use mimofan::models::{ContentBlock, MessageResponse};
use serde_json::{Value, json};

// ── anthropic_messages_url ──────────────────────────────────────────

#[test]
fn url_standard_anthropic_endpoint() {
    assert_eq!(
        anthropic_messages_url("https://api.anthropic.com"),
        "https://api.anthropic.com/v1/messages"
    );
}

#[test]
fn url_standard_anthropic_with_v1_suffix() {
    assert_eq!(
        anthropic_messages_url("https://api.anthropic.com/v1"),
        "https://api.anthropic.com/v1/messages"
    );
}

#[test]
fn url_xiaomimimo_anthropic_endpoint() {
    // XiaomiMiMo Anthropic endpoint: /anthropic → /anthropic/v1/messages
    assert_eq!(
        anthropic_messages_url("https://api.xiaomimimo.com/anthropic"),
        "https://api.xiaomimimo.com/anthropic/v1/messages"
    );
}

#[test]
fn url_xiaomimimo_anthropic_with_trailing_slash() {
    assert_eq!(
        anthropic_messages_url("https://api.xiaomimimo.com/anthropic/"),
        "https://api.xiaomimimo.com/anthropic/v1/messages"
    );
}

#[test]
fn url_bare_hostname_gets_v1_messages() {
    assert_eq!(
        anthropic_messages_url("https://custom-gateway.example.com"),
        "https://custom-gateway.example.com/v1/messages"
    );
}

#[test]
fn url_trailing_slashes_trimmed() {
    assert_eq!(
        anthropic_messages_url("https://api.anthropic.com///"),
        "https://api.anthropic.com/v1/messages"
    );
}

// ── anthropic_model_rejects_sampling ─────────────────────────────────

#[test]
fn sampling_rejected_for_opus_4_8() {
    assert!(anthropic_model_rejects_sampling("claude-opus-4-8"));
}

#[test]
fn sampling_rejected_for_fable() {
    assert!(anthropic_model_rejects_sampling("claude-fable-5"));
}

#[test]
fn sampling_allowed_for_sonnet() {
    assert!(!anthropic_model_rejects_sampling("claude-sonnet-4-6"));
}

#[test]
fn sampling_allowed_for_mimo() {
    assert!(!anthropic_model_rejects_sampling("mimo-v2.5-pro"));
}

// ── anthropic_tool_choice ────────────────────────────────────────────

#[test]
fn tool_choice_auto() {
    assert_eq!(
        anthropic_tool_choice(&json!("auto")),
        json!({ "type": "auto" })
    );
}

#[test]
fn tool_choice_none() {
    assert_eq!(
        anthropic_tool_choice(&json!("none")),
        json!({ "type": "none" })
    );
}

#[test]
fn tool_choice_any_object() {
    assert_eq!(
        anthropic_tool_choice(&json!({"type": "any"})),
        json!({ "type": "any" })
    );
}

#[test]
fn tool_choice_tool_name() {
    assert_eq!(
        anthropic_tool_choice(&json!({"type": "tool", "name": "bash"})),
        json!({ "type": "tool", "name": "bash" })
    );
}

#[test]
fn tool_choice_invalid_object_passthrough() {
    // Invalid object types are passed through as-is
    assert_eq!(
        anthropic_tool_choice(&json!({"type": "invalid"})),
        json!({ "type": "invalid" })
    );
}

// ── parse_anthropic_usage ────────────────────────────────────────────

#[test]
fn usage_from_anthropic_with_cache_fields() {
    let raw = json!({
        "input_tokens": 100,
        "output_tokens": 200,
        "cache_creation_input_tokens": 50,
        "cache_read_input_tokens": 30
    });
    let usage = parse_anthropic_usage(&raw);
    // input_tokens = raw + cache_creation + cache_read = 100 + 50 + 30 = 180
    assert_eq!(usage.input_tokens, 180);
    assert_eq!(usage.output_tokens, 200);
    assert_eq!(usage.prompt_cache_hit_tokens, Some(30));
    assert_eq!(usage.prompt_cache_miss_tokens, Some(150));
}

#[test]
fn usage_from_anthropic_without_cache_fields() {
    let raw = json!({
        "input_tokens": 100,
        "output_tokens": 200
    });
    let usage = parse_anthropic_usage(&raw);
    assert_eq!(usage.input_tokens, 100);
    assert_eq!(usage.output_tokens, 200);
    assert_eq!(usage.prompt_cache_hit_tokens, Some(0));
    assert_eq!(usage.prompt_cache_miss_tokens, Some(100));
}

#[test]
fn usage_from_xiaomimimo_minimal() {
    let raw = json!({
        "input_tokens": 55,
        "output_tokens": 101
    });
    let usage = parse_anthropic_usage(&raw);
    assert_eq!(usage.input_tokens, 55);
    assert_eq!(usage.output_tokens, 101);
    assert_eq!(usage.prompt_cache_hit_tokens, Some(0));
    assert_eq!(usage.prompt_cache_miss_tokens, Some(55));
}

// ── MessageResponse decoding ────────────────────────────────────────

#[test]
fn anthropic_live_response_decodes_to_message_response() {
    let mut value: Value = serde_json::from_str(ANTHROPIC_LIVE_RESPONSE).expect("json parse");
    if let Some(usage) = value.get_mut("usage") {
        *usage = json!(parse_anthropic_usage(usage));
    }
    let parsed: MessageResponse =
        serde_json::from_value(value).expect("Anthropic response must decode");

    assert_eq!(parsed.role, "assistant");
    assert_eq!(parsed.model, "claude-sonnet-4-6");
    assert_eq!(parsed.stop_reason.as_deref(), Some("end_turn"));
    assert_eq!(parsed.content.len(), 2);

    match &parsed.content[0] {
        ContentBlock::Text { text, .. } => {
            assert!(text.contains("Hello"));
        }
        other => panic!("expected text block, got {other:?}"),
    }
    match &parsed.content[1] {
        ContentBlock::ToolUse {
            id,
            name,
            input,
            caller: _,
        } => {
            assert!(!id.is_empty());
            assert_eq!(name, "bash");
            assert!(input.is_object());
        }
        other => panic!("expected tool_use block, got {other:?}"),
    }
}

#[test]
fn xiaomimimo_live_response_decodes_to_message_response() {
    let mut value: Value = serde_json::from_str(XIAOMIMIMO_LIVE_RESPONSE).expect("json parse");
    if let Some(usage) = value.get_mut("usage") {
        *usage = json!(parse_anthropic_usage(usage));
    }
    let parsed: MessageResponse =
        serde_json::from_value(value).expect("XiaomiMiMo Anthropic response must decode");

    assert_eq!(parsed.role, "assistant");
    assert_eq!(parsed.model, "mimo-v2.5-pro");
    assert_eq!(parsed.stop_reason.as_deref(), Some("end_turn"));
    assert_eq!(parsed.content.len(), 2);

    match &parsed.content[0] {
        ContentBlock::Text { text, .. } => {
            assert!(text.contains("MiMo"));
        }
        other => panic!("expected text block, got {other:?}"),
    }
    match &parsed.content[1] {
        ContentBlock::Thinking {
            thinking,
            signature,
        } => {
            assert!(!thinking.is_empty());
            assert_eq!(signature.as_deref(), Some(""));
        }
        other => panic!("expected thinking block, got {other:?}"),
    }

    // Usage normalized to the internal convention: input_tokens only
    // (no cache read), output_tokens passed through, cache fields
    // explicitly zeroed since the upstream payload omits them.
    assert_eq!(parsed.usage.input_tokens, 55);
    assert_eq!(parsed.usage.output_tokens, 101);
    assert_eq!(parsed.usage.prompt_cache_hit_tokens, Some(0));
    assert_eq!(parsed.usage.prompt_cache_miss_tokens, Some(55));
}

#[test]
fn xiaomimimo_endpoint_url_for_anthropic_provider() {
    // base_url from ~/.mimofan/config.toml providers.xiaomi_mimo →
    // `https://api.xiaomimimo.com/anthropic`
    assert_eq!(
        anthropic_messages_url("https://api.xiaomimimo.com/anthropic"),
        "https://api.xiaomimimo.com/anthropic/v1/messages"
    );
}

// ── apply_anthropic_cache_breakpoints ────────────────────────────────

#[test]
fn cache_breakpoints_placed_on_last_tool_and_last_user_block() {
    let mut body = json!({
        "tools": [
            {"name": "tool1", "description": "first", "input_schema": {}},
            {"name": "tool2", "description": "second", "input_schema": {}}
        ],
        "messages": [
            {"role": "user", "content": [{"type": "text", "text": "hello"}]},
            {"role": "assistant", "content": [{"type": "text", "text": "hi"}]},
            {"role": "user", "content": [
                {"type": "text", "text": "block1"},
                {"type": "text", "text": "block2"}
            ]}
        ]
    });
    apply_anthropic_cache_breakpoints(&mut body);

    // Last tool should have cache_control
    let tools = body["tools"]
        .as_array()
        .expect("convert JSON value to array");
    assert!(tools[1].get("cache_control").is_some());
    assert!(tools[0].get("cache_control").is_none());

    // Last block of last user message should have cache_control
    let messages = body["messages"]
        .as_array()
        .expect("convert JSON value to array");
    let last_user = &messages[2];
    let blocks = last_user["content"]
        .as_array()
        .expect("convert JSON value to array");
    assert!(blocks[1].get("cache_control").is_some());
    assert!(blocks[0].get("cache_control").is_none());
}

#[test]
fn cache_breakpoints_no_tools_uses_system() {
    let mut body = json!({
        "system": [
            {"type": "text", "text": "sys1"},
            {"type": "text", "text": "sys2"}
        ],
        "messages": [
            {"role": "user", "content": [{"type": "text", "text": "hi"}]}
        ]
    });
    apply_anthropic_cache_breakpoints(&mut body);

    let system = body["system"]
        .as_array()
        .expect("convert JSON value to array");
    assert!(system[1].get("cache_control").is_some());
    assert!(system[0].get("cache_control").is_none());
}

// ── Test fixtures ────────────────────────────────────────────────────

const ANTHROPIC_LIVE_RESPONSE: &str = r#"{
  "id": "msg_01XFDUDYJgAACzvnptvVoYEL",
  "type": "message",
  "role": "assistant",
  "content": [
    {
      "type": "text",
      "text": "Hello! I'm happy to help."
    },
    {
      "type": "tool_use",
      "id": "toolu_01A09q90qw90lq917835lq9",
      "name": "bash",
      "input": {"command": "ls"}
    }
  ],
  "model": "claude-sonnet-4-6",
  "stop_reason": "end_turn",
  "stop_sequence": null,
  "usage": {
    "input_tokens": 25,
    "output_tokens": 50
  }
}"#;
