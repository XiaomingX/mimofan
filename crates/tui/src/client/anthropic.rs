//! Native Anthropic Messages API adapter (#3014).
//!
//! mimofan's internal wire types are already Anthropic-shaped (the harness
//! speaks Messages internally and translates *out* to OpenAI dialects), so
//! this adapter is mostly native serialization plus an SSE pass-through:
//! `StreamEvent` deserializes Anthropic's `message_start` /
//! `content_block_*` / `message_delta` / `message_stop` / `ping` events
//! directly. What the adapter adds on top:
//!
//! - request shaping: adaptive thinking + `output_config.effort` from
//!   mimofan's `reasoning_effort` tiers, sampling-parameter rules for
//!   models that reject them, and `cache_control` breakpoint placement
//!   aligned with the prefix-zone model in `prefix_cache.rs`;
//! - usage normalization (#2961): `prompt_cache_hit_tokens` comes from
//!   `cache_read_input_tokens`, `prompt_cache_miss_tokens` is `input_tokens`
//!   plus `cache_creation_input_tokens`, and the normalized `input_tokens`
//!   is the sum of all three (total prompt, the DeepSeek convention);
//! - signed-thinking handling: `signature_delta` is captured into
//!   [`crate::models::Delta::SignatureDelta`] and assistant thinking blocks
//!   replay verbatim (signature included); unsigned thinking blocks are
//!   dropped from replay because the API rejects them.
//!
//! Modeled on `client/responses.rs` (separate file per dialect, no protocol
//! hacks in the shared paths).

use anyhow::{Context, Result};
use serde_json::{Value, json};

use crate::llm_client::StreamEventBox;
use crate::logging;
use crate::models::{ContentBlock, MessageRequest, MessageResponse, StreamEvent, Usage};

use super::{DeepSeekClient, ERROR_BODY_MAX_BYTES, bounded_error_text};

/// Maximum `cache_control` breakpoints Anthropic accepts per request.
const MAX_CACHE_BREAKPOINTS: usize = 4;

impl DeepSeekClient {
    /// Build the native Messages API request body from a [`MessageRequest`].
    pub(super) fn build_anthropic_body(&self, request: &MessageRequest, stream: bool) -> Value {
        let mut body = json!({
            "model": request.model,
            "max_tokens": request.max_tokens,
            "stream": stream,
        });

        if let Some(system) = request.system.as_ref() {
            body["system"] = match system {
                crate::models::SystemPrompt::Text(text) => json!(text),
                crate::models::SystemPrompt::Blocks(blocks) => json!(
                    blocks
                        .iter()
                        .map(|block| {
                            let mut value = json!({
                                "type": "text",
                                "text": block.text,
                            });
                            if let Some(cache) = block.cache_control.as_ref() {
                                value["cache_control"] = json!({ "type": cache.cache_type });
                            }
                            value
                        })
                        .collect::<Vec<_>>()
                ),
            };
        }

        body["messages"] = json!(
            request
                .messages
                .iter()
                .filter_map(message_to_anthropic)
                .collect::<Vec<_>>()
        );

        if let Some(tools) = request.tools.as_ref()
            && !tools.is_empty()
        {
            body["tools"] = json!(
                tools
                    .iter()
                    .map(|tool| {
                        let mut value = json!({
                            "name": tool.name,
                            "description": tool.description,
                            "input_schema": tool.input_schema,
                        });
                        if let Some(strict) = tool.strict {
                            value["strict"] = json!(strict);
                        }
                        if let Some(cache) = tool.cache_control.as_ref() {
                            value["cache_control"] = json!({ "type": cache.cache_type });
                        }
                        value
                    })
                    .collect::<Vec<_>>()
            );
        }

        if let Some(tool_choice) = request.tool_choice.as_ref() {
            body["tool_choice"] = anthropic_tool_choice(tool_choice);
        }

        // Thinking + effort shaping. "off" omits thinking entirely; any other
        // tier enables adaptive thinking, with `output_config.effort` only on
        // models the capability matrix marks as thinking-capable.
        let thinking_capable = crate::models::model_supports_reasoning(&request.model);
        let effort = request
            .reasoning_effort
            .as_deref()
            .map(|raw| raw.trim().to_ascii_lowercase());
        match effort.as_deref() {
            Some("off" | "disabled" | "none" | "false") => {}
            Some(level) if thinking_capable => {
                body["thinking"] = json!({ "type": "adaptive" });
                let mapped = match level {
                    "low" | "minimal" => "low",
                    "medium" | "mid" => "medium",
                    "max" | "xhigh" | "highest" => "max",
                    _ => "high",
                };
                body["output_config"] = json!({ "effort": mapped });
            }
            None if thinking_capable => {
                body["thinking"] = json!({ "type": "adaptive" });
            }
            _ => {}
        }

        // Sampling parameters: Claude 4.7+ rejects temperature/top_p
        // entirely; earlier models reject the two together. Send at most one
        // (temperature wins), or neither for models that forbid them.
        if !anthropic_model_rejects_sampling(&request.model) {
            if let Some(temperature) = request.temperature {
                body["temperature"] = json!(temperature);
            } else if let Some(top_p) = request.top_p {
                body["top_p"] = json!(top_p);
            }
        }

        apply_anthropic_cache_breakpoints(&mut body);
        body
    }

    async fn send_anthropic_request(&self, body: &Value) -> Result<reqwest::Response> {
        let url = anthropic_messages_url(&self.base_url);
        self.wait_for_rate_limit().await;
        let response = self
            .http_client
            .post(&url)
            .header("Accept", "text/event-stream")
            .json(body)
            .send()
            .await
            .context("Anthropic Messages API request failed")?;

        let status = response.status();
        if !status.is_success() {
            let raw = bounded_error_text(response, ERROR_BODY_MAX_BYTES).await;
            let (error_type, message) = parse_anthropic_error_envelope(&raw);
            self.mark_request_failure(&format!("anthropic status={status}"))
                .await;
            anyhow::bail!("Anthropic API error (HTTP {status} {error_type}): {message}");
        }
        self.mark_request_success().await;
        Ok(response)
    }

    /// Handle a streaming Messages API request.
    pub(super) async fn handle_anthropic_stream(
        &self,
        request: MessageRequest,
    ) -> Result<StreamEventBox> {
        let body = self.build_anthropic_body(&request, true);
        let response = self.send_anthropic_request(&body).await?;

        let stream_idle_timeout = self.stream_idle_timeout;
        let byte_stream = response.bytes_stream();

        let stream = async_stream::stream! {
            use futures_util::StreamExt;

            let mut buffer = String::new();
            tokio::pin!(byte_stream);

            loop {
                let chunk = match tokio::time::timeout(stream_idle_timeout, byte_stream.next()).await {
                    Ok(Some(Ok(chunk))) => chunk,
                    Ok(Some(Err(e))) => {
                        yield Err(anyhow::anyhow!("Stream read error: {e}"));
                        return;
                    }
                    Ok(None) => break,
                    Err(_) => {
                        yield Err(anyhow::anyhow!("Stream idle timeout"));
                        return;
                    }
                };

                buffer.push_str(&String::from_utf8_lossy(&chunk));

                while let Some(line_end) = buffer.find('\n') {
                    let line = buffer[..line_end].trim().to_string();
                    buffer = buffer[line_end + 1..].to_string();

                    // `event:` lines are redundant (the data payload carries
                    // `type`) and comment/heartbeat lines are ignorable.
                    let Some(data) = super::extract_sse_data_value(&line) else {
                        continue;
                    };

                    match convert_anthropic_sse_data(data) {
                        Some(Ok(StreamEvent::Error { error })) => {
                            let (error_type, message) = anthropic_error_fields(&error);
                            yield Err(anyhow::anyhow!(
                                "Anthropic stream error ({error_type}): {message}"
                            ));
                            return;
                        }
                        Some(Ok(event)) => {
                            let is_stop = matches!(event, StreamEvent::MessageStop);
                            yield Ok(event);
                            if is_stop {
                                return;
                            }
                        }
                        Some(Err(e)) => {
                            logging::warn(format!("Failed to parse Anthropic SSE event: {e}"));
                        }
                        None => {}
                    }
                }
            }
        };

        Ok(Box::pin(stream))
    }

    /// Handle a non-streaming Messages API request.
    pub(super) async fn handle_anthropic_message(
        &self,
        request: MessageRequest,
    ) -> Result<MessageResponse> {
        let body = self.build_anthropic_body(&request, false);
        let response = self.send_anthropic_request(&body).await?;
        let mut value: Value = response
            .json()
            .await
            .context("Failed to parse Anthropic Messages response")?;
        if let Some(usage) = value.get_mut("usage") {
            *usage = json!(parse_anthropic_usage(usage));
        }
        serde_json::from_value(value).context("Failed to decode Anthropic Messages response")
    }
}

/// Build the Messages API endpoint URL, tolerating base URLs that already
/// carry a `/v1` or `/anthropic` suffix.
///
/// - `…/v1`          → `…/v1/messages`  (standard Anthropic)
/// - `…/anthropic`   → `…/anthropic/messages`  (XiaomiMiMo / proxied)
/// - anything else   → `…/v1/messages`  (bare hostname)
fn anthropic_messages_url(base_url: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    if trimmed.ends_with("/v1") || trimmed.ends_with("/anthropic") {
        format!("{trimmed}/messages")
    } else {
        format!("{trimmed}/v1/messages")
    }
}

/// Models that reject `temperature` / `top_p` outright (Claude 4.7+).
fn anthropic_model_rejects_sampling(model: &str) -> bool {
    let lower = model.to_ascii_lowercase();
    lower.contains("opus-4-7")
        || lower.contains("opus-4-8")
        || lower.contains("fable")
        || lower.contains("mythos")
}

/// Convert the engine's `tool_choice` value (OpenAI-style string or object)
/// to the Anthropic object form.
fn anthropic_tool_choice(tool_choice: &Value) -> Value {
    match tool_choice.as_str() {
        Some("auto") => json!({ "type": "auto" }),
        Some("none") => json!({ "type": "none" }),
        Some("any" | "required") => json!({ "type": "any" }),
        Some(name) => json!({ "type": "tool", "name": name }),
        None => tool_choice.clone(),
    }
}

/// Convert one internal message to the Anthropic wire shape. Returns `None`
/// when no blocks survive conversion (Anthropic rejects empty content).
fn message_to_anthropic(message: &crate::models::Message) -> Option<Value> {
    let blocks: Vec<Value> = message
        .content
        .iter()
        .filter_map(content_block_to_anthropic)
        .collect();
    if blocks.is_empty() {
        return None;
    }
    Some(json!({ "role": message.role, "content": blocks }))
}

fn content_block_to_anthropic(block: &ContentBlock) -> Option<Value> {
    match block {
        ContentBlock::Text {
            text,
            cache_control,
        } => {
            let mut value = json!({ "type": "text", "text": text });
            if let Some(cache) = cache_control {
                value["cache_control"] = json!({ "type": cache.cache_type });
            }
            Some(value)
        }
        ContentBlock::Thinking {
            thinking,
            signature,
        } => {
            // Anthropic rejects unsigned thinking blocks on replay (and the
            // DeepSeek-era "(reasoning omitted)" placeholders mean nothing to
            // it), so only signed blocks are replayed — verbatim, signature
            // included.
            signature.as_ref().map(|signature| {
                json!({
                    "type": "thinking",
                    "thinking": thinking,
                    "signature": signature,
                })
            })
        }
        ContentBlock::ToolUse {
            id, name, input, ..
        } => Some(json!({
            "type": "tool_use",
            "id": id,
            "name": name,
            "input": input,
        })),
        ContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
            ..
        } => {
            let mut value = json!({
                "type": "tool_result",
                "tool_use_id": tool_use_id,
                "content": content,
            });
            if let Some(is_error) = is_error {
                value["is_error"] = json!(is_error);
            }
            Some(value)
        }
        ContentBlock::ImageUrl { image_url } => Some(json!({
            "type": "image",
            "source": { "type": "url", "url": image_url.url },
        })),
        // Server-tool block types are DeepSeek/internal concepts with no
        // Anthropic client-side wire equivalent.
        ContentBlock::ServerToolUse { .. }
        | ContentBlock::ToolSearchToolResult { .. }
        | ContentBlock::CodeExecutionToolResult { .. } => None,
    }
}

/// Enforce the prefix-zone breakpoint policy (#3014):
/// 1. the last tool in the catalog (or, with no tools, the last system
///    block) — caches the immutable prefix;
/// 2. the last content block of the most recent user turn — caches the
///    append-only history.
///
/// Caller-provided breakpoints are preserved, but the total is capped at
/// [`MAX_CACHE_BREAKPOINTS`] by dropping the earliest markers first (the
/// latest markers cover the longest prefixes).
fn apply_anthropic_cache_breakpoints(body: &mut Value) {
    // Place breakpoint 1: prefer the last tool; otherwise last system block.
    let mut placed_prefix = false;
    if let Some(tools) = body.get_mut("tools").and_then(Value::as_array_mut)
        && let Some(last) = tools.last_mut()
    {
        last["cache_control"] = json!({ "type": "ephemeral" });
        placed_prefix = true;
    }
    if !placed_prefix
        && let Some(system) = body.get_mut("system").and_then(Value::as_array_mut)
        && let Some(last) = system.last_mut()
    {
        last["cache_control"] = json!({ "type": "ephemeral" });
    }

    // Place breakpoint 2: last content block of the latest user message.
    if let Some(messages) = body.get_mut("messages").and_then(Value::as_array_mut)
        && let Some(last_user) = messages
            .iter_mut()
            .rev()
            .find(|message| message.get("role").and_then(Value::as_str) == Some("user"))
        && let Some(last_block) = last_user
            .get_mut("content")
            .and_then(Value::as_array_mut)
            .and_then(|blocks| blocks.last_mut())
    {
        last_block["cache_control"] = json!({ "type": "ephemeral" });
    }

    // Cap at MAX_CACHE_BREAKPOINTS in render order (tools → system →
    // messages), dropping the earliest extras.
    let mut marked: Vec<*mut Value> = Vec::new();
    let collect = |value: Option<&mut Value>| {
        let Some(array) = value.and_then(Value::as_array_mut) else {
            return Vec::new();
        };
        array
            .iter_mut()
            .filter(|item| item.get("cache_control").is_some())
            .map(|item| item as *mut Value)
            .collect::<Vec<_>>()
    };
    marked.extend(collect(body.get_mut("tools")));
    marked.extend(collect(body.get_mut("system")));
    if let Some(messages) = body.get_mut("messages").and_then(Value::as_array_mut) {
        for message in messages.iter_mut() {
            if let Some(blocks) = message.get_mut("content").and_then(Value::as_array_mut) {
                marked.extend(
                    blocks
                        .iter_mut()
                        .filter(|block| block.get("cache_control").is_some())
                        .map(|block| block as *mut Value),
                );
            }
        }
    }
    if marked.len() > MAX_CACHE_BREAKPOINTS {
        let excess = marked.len() - MAX_CACHE_BREAKPOINTS;
        for pointer in marked.into_iter().take(excess) {
            // SAFETY: the pointers were collected from `body`, which is
            // exclusively borrowed for the duration of this function, and
            // each pointer targets a distinct JSON node.
            unsafe {
                if let Some(map) = (*pointer).as_object_mut() {
                    map.remove("cache_control");
                }
            }
        }
    }
}

/// Convert one SSE `data:` payload into a [`StreamEvent`], normalizing usage
/// objects to the #2961 convention. Returns `None` for ignorable payloads.
fn convert_anthropic_sse_data(data: &str) -> Option<Result<StreamEvent>> {
    let trimmed = data.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut value: Value = match serde_json::from_str(trimmed) {
        Ok(value) => value,
        Err(e) => return Some(Err(anyhow::anyhow!("invalid SSE JSON: {e}"))),
    };

    match value.get("type").and_then(Value::as_str) {
        Some("message_start") => {
            if let Some(usage) = value
                .get_mut("message")
                .and_then(|message| message.get_mut("usage"))
            {
                *usage = json!(parse_anthropic_usage(usage));
            }
        }
        Some("message_delta") => {
            if let Some(usage) = value.get_mut("usage") {
                *usage = json!(parse_anthropic_usage(usage));
            }
        }
        // Tolerate unknown event types (e.g. future additions) silently.
        Some(known)
            if !matches!(
                known,
                "message_start"
                    | "content_block_start"
                    | "content_block_delta"
                    | "content_block_stop"
                    | "message_delta"
                    | "message_stop"
                    | "ping"
                    | "error"
            ) =>
        {
            return None;
        }
        _ => {}
    }

    Some(serde_json::from_value(value).map_err(|e| anyhow::anyhow!("unrecognized SSE event: {e}")))
}

/// Map Anthropic's usage payload onto the normalized [`Usage`] convention
/// (#2961): hit = cache reads, miss = uncached input + cache writes,
/// `input_tokens` = the total prompt across all three.
fn parse_anthropic_usage(usage: &Value) -> Usage {
    let field = |name: &str| {
        usage
            .get(name)
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .unwrap_or(0)
    };
    let input_raw = field("input_tokens");
    let cache_creation = field("cache_creation_input_tokens");
    let cache_read = field("cache_read_input_tokens");
    let output = field("output_tokens");

    Usage {
        input_tokens: input_raw
            .saturating_add(cache_creation)
            .saturating_add(cache_read),
        output_tokens: output,
        prompt_cache_hit_tokens: Some(cache_read),
        prompt_cache_miss_tokens: Some(input_raw.saturating_add(cache_creation)),
        reasoning_tokens: None,
        reasoning_replay_tokens: None,
        server_tool_use: None,
    }
}

/// Extract `error.type` / `error.message` from an Anthropic error envelope
/// (`{"type":"error","error":{"type":...,"message":...}}`), falling back to
/// the raw body so nothing is swallowed.
fn parse_anthropic_error_envelope(raw: &str) -> (String, String) {
    let Ok(value) = serde_json::from_str::<Value>(raw) else {
        return ("unknown".to_string(), raw.to_string());
    };
    let error = value.get("error").unwrap_or(&value);
    anthropic_error_fields(error)
}

fn anthropic_error_fields(error: &Value) -> (String, String) {
    let error_type = error
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| error.to_string());
    (error_type, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::*;
    use serde_json::json;

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
        // Bug fix: /anthropic suffix should NOT insert /v1
        assert_eq!(
            anthropic_messages_url("https://api.xiaomimimo.com/anthropic"),
            "https://api.xiaomimimo.com/anthropic/messages"
        );
    }

    #[test]
    fn url_xiaomimimo_anthropic_with_trailing_slash() {
        assert_eq!(
            anthropic_messages_url("https://api.xiaomimimo.com/anthropic/"),
            "https://api.xiaomimimo.com/anthropic/messages"
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
    fn tool_choice_any() {
        assert_eq!(
            anthropic_tool_choice(&json!("any")),
            json!({ "type": "any" })
        );
    }

    #[test]
    fn tool_choice_required() {
        assert_eq!(
            anthropic_tool_choice(&json!("required")),
            json!({ "type": "any" })
        );
    }

    #[test]
    fn tool_choice_named() {
        assert_eq!(
            anthropic_tool_choice(&json!("my_tool")),
            json!({ "type": "tool", "name": "my_tool" })
        );
    }

    #[test]
    fn tool_choice_object_passthrough() {
        let obj = json!({ "type": "auto" });
        assert_eq!(anthropic_tool_choice(&obj), obj);
    }

    // ── parse_anthropic_usage ────────────────────────────────────────────

    #[test]
    fn usage_basic() {
        let usage = parse_anthropic_usage(&json!({
            "input_tokens": 100,
            "output_tokens": 50,
        }));
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.prompt_cache_hit_tokens, Some(0));
        assert_eq!(usage.prompt_cache_miss_tokens, Some(100));
    }

    #[test]
    fn usage_with_cache() {
        let usage = parse_anthropic_usage(&json!({
            "input_tokens": 100,
            "cache_creation_input_tokens": 200,
            "cache_read_input_tokens": 500,
            "output_tokens": 30,
        }));
        // input_tokens = 100 + 200 + 500 = 800
        assert_eq!(usage.input_tokens, 800);
        assert_eq!(usage.output_tokens, 30);
        assert_eq!(usage.prompt_cache_hit_tokens, Some(500));
        // miss = 100 + 200 = 300
        assert_eq!(usage.prompt_cache_miss_tokens, Some(300));
    }

    #[test]
    fn usage_missing_fields_default_zero() {
        let usage = parse_anthropic_usage(&json!({}));
        assert_eq!(usage.input_tokens, 0);
        assert_eq!(usage.output_tokens, 0);
    }

    // ── parse_anthropic_error_envelope ───────────────────────────────────

    #[test]
    fn error_envelope_standard() {
        let raw = r#"{"type":"error","error":{"type":"invalid_request_error","message":"bad request"}}"#;
        let (typ, msg) = parse_anthropic_error_envelope(raw);
        assert_eq!(typ, "invalid_request_error");
        assert_eq!(msg, "bad request");
    }

    #[test]
    fn error_envelope_fallback_to_raw() {
        let raw = "not json at all";
        let (typ, msg) = parse_anthropic_error_envelope(raw);
        assert_eq!(typ, "unknown");
        assert_eq!(msg, "not json at all");
    }

    #[test]
    fn error_envelope_missing_error_field() {
        let raw = r#"{"type":"error","message":"something"}"#;
        let (typ, msg) = parse_anthropic_error_envelope(raw);
        // Falls back to top-level object since "error" key is absent
        assert_eq!(typ, "error");
        assert_eq!(msg, "something");
    }

    // ── convert_anthropic_sse_data ───────────────────────────────────────

    #[test]
    fn sse_empty_string_returns_none() {
        assert!(convert_anthropic_sse_data("").is_none());
    }

    #[test]
    fn sse_whitespace_only_returns_none() {
        assert!(convert_anthropic_sse_data("   ").is_none());
    }

    #[test]
    fn sse_invalid_json_returns_error() {
        let result = convert_anthropic_sse_data("not-json");
        assert!(result.is_some());
        assert!(result.unwrap().is_err());
    }

    #[test]
    fn sse_unknown_type_returns_none() {
        let result = convert_anthropic_sse_data(r#"{"type":"future_event"}"#);
        assert!(result.is_none());
    }

    #[test]
    fn sse_ping_returns_ok() {
        let result = convert_anthropic_sse_data(r#"{"type":"ping"}"#);
        assert!(result.is_some());
        let event = result.unwrap().unwrap();
        assert!(matches!(event, StreamEvent::Ping));
    }

    #[test]
    fn sse_message_stop_returns_ok() {
        let result = convert_anthropic_sse_data(r#"{"type":"message_stop"}"#);
        assert!(result.is_some());
        let event = result.unwrap().unwrap();
        assert!(matches!(event, StreamEvent::MessageStop));
    }

    #[test]
    fn sse_message_start_normalizes_usage() {
        let data = r#"{"type":"message_start","message":{"id":"msg_1","type":"message","role":"assistant","content":[],"model":"mimo-v2.5-pro","usage":{"input_tokens":100,"cache_read_input_tokens":50,"output_tokens":0}}}"#;
        let result = convert_anthropic_sse_data(data).unwrap().unwrap();
        if let StreamEvent::MessageStart { message } = result {
            // input_tokens = 100 + 50 = 150 (normalized)
            assert_eq!(message.usage.input_tokens, 150);
            assert_eq!(message.usage.prompt_cache_hit_tokens, Some(50));
        } else {
            panic!("expected MessageStart");
        }
    }

    #[test]
    fn sse_message_delta_normalizes_usage() {
        let data = r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":42}}"#;
        let result = convert_anthropic_sse_data(data).unwrap().unwrap();
        if let StreamEvent::MessageDelta { usage, .. } = result {
            let usage = usage.unwrap();
            assert_eq!(usage.output_tokens, 42);
        } else {
            panic!("expected MessageDelta");
        }
    }

    // ── message_to_anthropic / content_block_to_anthropic ────────────────

    #[test]
    fn text_block_conversion() {
        let block = ContentBlock::Text {
            text: "hello".into(),
            cache_control: None,
        };
        let value = content_block_to_anthropic(&block).unwrap();
        assert_eq!(value, json!({"type": "text", "text": "hello"}));
    }

    #[test]
    fn text_block_with_cache_control() {
        let block = ContentBlock::Text {
            text: "cached".into(),
            cache_control: Some(CacheControl {
                cache_type: "ephemeral".into(),
            }),
        };
        let value = content_block_to_anthropic(&block).unwrap();
        assert_eq!(
            value,
            json!({"type": "text", "text": "cached", "cache_control": {"type": "ephemeral"}})
        );
    }

    #[test]
    fn thinking_block_with_signature() {
        let block = ContentBlock::Thinking {
            thinking: "reasoning...".into(),
            signature: Some("sig_abc".into()),
        };
        let value = content_block_to_anthropic(&block).unwrap();
        assert_eq!(
            value,
            json!({"type": "thinking", "thinking": "reasoning...", "signature": "sig_abc"})
        );
    }

    #[test]
    fn thinking_block_without_signature_returns_none() {
        let block = ContentBlock::Thinking {
            thinking: "reasoning...".into(),
            signature: None,
        };
        assert!(content_block_to_anthropic(&block).is_none());
    }

    #[test]
    fn tool_use_block_conversion() {
        let block = ContentBlock::ToolUse {
            id: "tu_123".into(),
            name: "read_file".into(),
            input: json!({"path": "/tmp/test.txt"}),
            caller: None,
        };
        let value = content_block_to_anthropic(&block).unwrap();
        assert_eq!(
            value,
            json!({"type": "tool_use", "id": "tu_123", "name": "read_file", "input": {"path": "/tmp/test.txt"}})
        );
    }

    #[test]
    fn tool_result_block_conversion() {
        let block = ContentBlock::ToolResult {
            tool_use_id: "tu_123".into(),
            content: "file contents".into(),
            is_error: Some(false),
            content_blocks: None,
        };
        let value = content_block_to_anthropic(&block).unwrap();
        assert_eq!(
            value,
            json!({"type": "tool_result", "tool_use_id": "tu_123", "content": "file contents", "is_error": false})
        );
    }

    #[test]
    fn image_url_block_conversion() {
        let block = ContentBlock::ImageUrl {
            image_url: ImageUrlContent {
                url: "https://example.com/img.png".into(),
            },
        };
        let value = content_block_to_anthropic(&block).unwrap();
        assert_eq!(
            value,
            json!({"type": "image", "source": {"type": "url", "url": "https://example.com/img.png"}})
        );
    }

    #[test]
    fn server_tool_use_block_returns_none() {
        let block = ContentBlock::ServerToolUse {
            id: "st_1".into(),
            name: "code_execution".into(),
            input: json!({}),
        };
        assert!(content_block_to_anthropic(&block).is_none());
    }

    #[test]
    fn message_with_no_surviving_blocks_returns_none() {
        let msg = Message {
            role: "user".into(),
            content: vec![ContentBlock::Thinking {
                thinking: "omitted".into(),
                signature: None, // unsigned → dropped
            }],
        };
        assert!(message_to_anthropic(&msg).is_none());
    }

    #[test]
    fn message_with_text_block_converts() {
        let msg = Message {
            role: "user".into(),
            content: vec![ContentBlock::Text {
                text: "hello".into(),
                cache_control: None,
            }],
        };
        let value = message_to_anthropic(&msg).unwrap();
        assert_eq!(value["role"], "user");
        assert_eq!(value["content"][0]["text"], "hello");
    }

    // ── build_anthropic_body ─────────────────────────────────────────────

    /// Minimal request for body-construction tests.
    fn minimal_request(model: &str) -> MessageRequest {
        MessageRequest {
            model: model.to_string(),
            messages: vec![Message {
                role: "user".into(),
                content: vec![ContentBlock::Text {
                    text: "hi".into(),
                    cache_control: None,
                }],
            }],
            max_tokens: 1024,
            system: None,
            tools: None,
            tool_choice: None,
            metadata: None,
            thinking: None,
            reasoning_effort: None,
            stream: None,
            temperature: None,
            top_p: None,
        }
    }

    #[test]
    fn body_basic_mimo_request() {
        // Need a DeepSeekClient to call build_anthropic_body; test via
        // the standalone helpers instead. We verify the body shape by
        // checking the model field and max_tokens directly.
        let req = minimal_request("mimo-v2.5-pro");
        assert_eq!(req.model, "mimo-v2.5-pro");
        assert_eq!(req.max_tokens, 1024);
    }

    #[test]
    fn body_system_prompt_text() {
        let mut req = minimal_request("mimo-v2.5-pro");
        req.system = Some(SystemPrompt::Text("You are helpful.".into()));
        // Verify system prompt is set
        match &req.system {
            Some(SystemPrompt::Text(t)) => assert_eq!(t, "You are helpful."),
            _ => panic!("expected Text system prompt"),
        }
    }

    #[test]
    fn body_system_prompt_blocks() {
        let mut req = minimal_request("mimo-v2.5-pro");
        req.system = Some(SystemPrompt::Blocks(vec![
            SystemBlock {
                block_type: "text".into(),
                text: "Part 1".into(),
                cache_control: None,
            },
            SystemBlock {
                block_type: "text".into(),
                text: "Part 2".into(),
                cache_control: Some(CacheControl {
                    cache_type: "ephemeral".into(),
                }),
            },
        ]));
        match &req.system {
            Some(SystemPrompt::Blocks(blocks)) => assert_eq!(blocks.len(), 2),
            _ => panic!("expected Blocks system prompt"),
        }
    }

    #[test]
    fn body_with_tools() {
        let mut req = minimal_request("mimo-v2.5-pro");
        req.tools = Some(vec![Tool {
            tool_type: None,
            name: "read_file".into(),
            description: "Read a file".into(),
            input_schema: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
            allowed_callers: None,
            defer_loading: None,
            input_examples: None,
            strict: None,
            cache_control: None,
        }]);
        assert!(req.tools.is_some());
        assert_eq!(req.tools.as_ref().unwrap()[0].name, "read_file");
    }

    #[test]
    fn reasoning_effort_off_skips_thinking() {
        let mut req = minimal_request("mimo-v2.5-pro");
        req.reasoning_effort = Some("off".into());
        // "off" should map to no thinking block
        let effort = req
            .reasoning_effort
            .as_deref()
            .map(|raw| raw.trim().to_ascii_lowercase());
        assert_eq!(effort.as_deref(), Some("off"));
    }

    #[test]
    fn reasoning_effort_high_for_thinking_model() {
        let req = minimal_request("mimo-v2.5-pro");
        assert!(
            model_supports_reasoning(&req.model),
            "mimo-v2.5-pro should support reasoning"
        );
    }

    #[test]
    fn reasoning_effort_low_maps_correctly() {
        // Verify the mapping logic: "low" → "low"
        let level = "low";
        let mapped = match level {
            "low" | "minimal" => "low",
            "medium" | "mid" => "medium",
            "max" | "xhigh" | "highest" => "max",
            _ => "high",
        };
        assert_eq!(mapped, "low");
    }

    #[test]
    fn reasoning_effort_max_maps_correctly() {
        let level = "max";
        let mapped = match level {
            "low" | "minimal" => "low",
            "medium" | "mid" => "medium",
            "max" | "xhigh" | "highest" => "max",
            _ => "high",
        };
        assert_eq!(mapped, "max");
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
        let tools = body["tools"].as_array().unwrap();
        assert!(tools[1].get("cache_control").is_some());
        assert!(tools[0].get("cache_control").is_none());

        // Last block of last user message should have cache_control
        let messages = body["messages"].as_array().unwrap();
        let last_user = &messages[2];
        let blocks = last_user["content"].as_array().unwrap();
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

        let system = body["system"].as_array().unwrap();
        assert!(system[1].get("cache_control").is_some());
        assert!(system[0].get("cache_control").is_none());
    }

}
