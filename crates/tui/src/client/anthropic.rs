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
use crate::models::{ContentBlock, Message, MessageRequest, MessageResponse, StreamEvent, Usage};

use super::{ApiClient, ERROR_BODY_MAX_BYTES, bounded_error_text};

/// Maximum `cache_control` breakpoints Anthropic accepts per request.
const MAX_CACHE_BREAKPOINTS: usize = 4;

impl ApiClient {
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
            project_messages_for_anthropic(&request.messages)
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

    async fn send_anthropic_request(
        &self,
        body: &Value,
        stream: bool,
    ) -> Result<reqwest::Response> {
        let url = anthropic_messages_url(&self.base_url);
        self.wait_for_rate_limit().await;
        // Pin the Accept header to the wire format we're asking for. The
        // XiaomiMiMo `/anthropic` gateway keys the response shape off the
        // Accept header (not the body's `stream` field): it always returns
        // SSE-wrapped `data: {...}` frames when the client advertises
        // `text/event-stream`, even for non-streaming calls. Pinning
        // `application/json` for non-streaming requests keeps the body
        // directly parseable; the standard Anthropic Messages API behaves
        // the same way.
        let accept = if stream {
            "text/event-stream"
        } else {
            "application/json"
        };
        let response = self
            .http_client
            .post(&url)
            .header("Accept", accept)
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
        let response = self.send_anthropic_request(&body, true).await?;

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
        let response = self.send_anthropic_request(&body, false).await?;
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
/// carry a `/v1` suffix.
///
/// - `…/v1`          → `…/v1/messages`  (standard Anthropic)
/// - `…/anthropic`   → `…/anthropic/v1/messages`  (XiaomiMiMo)
/// - anything else   → `…/v1/messages`  (bare hostname)
pub fn anthropic_messages_url(base_url: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    if trimmed.ends_with("/v1") {
        format!("{trimmed}/messages")
    } else {
        format!("{trimmed}/v1/messages")
    }
}

/// Models that reject `temperature` / `top_p` outright (Claude 4.7+).
pub fn anthropic_model_rejects_sampling(model: &str) -> bool {
    let lower = model.to_ascii_lowercase();
    lower.contains("opus-4-7")
        || lower.contains("opus-4-8")
        || lower.contains("fable")
        || lower.contains("mythos")
}

/// Convert the engine's `tool_choice` value (OpenAI-style string or object)
/// to the Anthropic object form.
pub fn anthropic_tool_choice(tool_choice: &Value) -> Value {
    match tool_choice.as_str() {
        Some("auto") => json!({ "type": "auto" }),
        Some("none") => json!({ "type": "none" }),
        Some("any" | "required") => json!({ "type": "any" }),
        Some(name) => json!({ "type": "tool", "name": name }),
        None => tool_choice.clone(),
    }
}

/// 在发送给 Anthropic 前对历史做一致性投影（对照 OpenAI 链 chat.rs 的既有清洗）：
///
/// - **孤儿 tool_result**：`tool_result` 的 `tool_use_id` 在全部历史中找不到对应
///   `tool_use`（例如 compaction 已把调用压缩掉）时丢弃该 block，避免 Anthropic 拒绝。
/// - **首条非 user**：Anthropic 要求消息列表首条为 `user`；若历史开头是非 user
///   （assistant / tool），丢弃开头的非 user 消息直到首条是 user。
///
/// 不做重复内容折叠（Anthropic 链无 OpenAI 的 `role:"tool"` 语义，重复由上游
/// compaction 的 `enforce_tool_call_pairs` 负责）。投影保持消息顺序与其余 block 不变。
fn project_messages_for_anthropic(messages: &[Message]) -> Vec<Message> {
    // 1. 全局收集所有 tool_use id。
    let tool_use_ids: std::collections::HashSet<&str> = messages
        .iter()
        .flat_map(|m| m.content.iter())
        .filter_map(|b| match b {
            ContentBlock::ToolUse { id, .. } => Some(id.as_str()),
            _ => None,
        })
        .collect();

    // 2. 逐消息重建 content，丢弃孤儿 tool_result；跳过空 content 的消息。
    let mut orphan_tool_results = 0usize;
    let mut projected: Vec<Message> = messages
        .iter()
        .map(|m| {
            let content: Vec<ContentBlock> = m
                .content
                .iter()
                .filter(|b| match b {
                    ContentBlock::ToolResult { tool_use_id, .. } => {
                        let known = tool_use_ids.contains(tool_use_id.as_str());
                        if !known {
                            orphan_tool_results += 1;
                        }
                        known
                    }
                    _ => true,
                })
                .cloned()
                .collect();
            Message {
                role: m.role.clone(),
                content,
            }
        })
        .filter(|m| !m.content.is_empty())
        .collect();

    // 3. 首条非 user 修正：丢弃开头的非 user 消息。
    let mut dropped_leading = 0usize;
    while projected.first().is_some_and(|m| m.role != "user") {
        projected.remove(0);
        dropped_leading += 1;
    }

    // 4. Anomaly 上报：投影消除的一致性异常本不应出现（上游 compaction 应已
    //    配对工具调用与结果、首条应为 user）。记录为 anomaly 以便诊断历史污染
    //    的根因，而不是静默吞掉。
    if orphan_tool_results > 0 || dropped_leading > 0 {
        crate::logging::warn(format!(
            "anthropic history anomaly: dropped {orphan_tool_results} orphan tool_result(s), \
             {dropped_leading} leading non-user message(s)"
        ));
    }

    projected
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
pub fn apply_anthropic_cache_breakpoints(body: &mut Value) {
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
    // Two-pass approach: first count marked items, then remove excess by index.
    let tools_count = body
        .get("tools")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter(|item| item.get("cache_control").is_some())
                .count()
        })
        .unwrap_or(0);
    let system_count = body
        .get("system")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter(|item| item.get("cache_control").is_some())
                .count()
        })
        .unwrap_or(0);
    let mut message_marked: Vec<(usize, usize)> = Vec::new(); // (msg_idx, block_idx)
    if let Some(messages) = body.get("messages").and_then(Value::as_array) {
        for (mi, message) in messages.iter().enumerate() {
            if let Some(blocks) = message.get("content").and_then(Value::as_array) {
                for (bi, block) in blocks.iter().enumerate() {
                    if block.get("cache_control").is_some() {
                        message_marked.push((mi, bi));
                    }
                }
            }
        }
    }
    let total = tools_count + system_count + message_marked.len();
    if total <= MAX_CACHE_BREAKPOINTS {
        return;
    }
    let excess = total - MAX_CACHE_BREAKPOINTS;
    // Remove from the earliest items first: tools → system → messages.
    let mut to_remove = excess;
    // Remove from tools array (first `to_remove` marked items).
    if to_remove > 0
        && let Some(tools) = body.get_mut("tools").and_then(Value::as_array_mut)
    {
        let mut removed = 0;
        for item in tools.iter_mut() {
            if removed >= to_remove {
                break;
            }
            if item.get("cache_control").is_some() {
                if let Some(map) = item.as_object_mut() {
                    map.remove("cache_control");
                }
                removed += 1;
            }
        }
        to_remove -= removed;
    }
    // Remove from system array.
    if to_remove > 0
        && let Some(system) = body.get_mut("system").and_then(Value::as_array_mut)
    {
        let mut removed = 0;
        for item in system.iter_mut() {
            if removed >= to_remove {
                break;
            }
            if item.get("cache_control").is_some() {
                if let Some(map) = item.as_object_mut() {
                    map.remove("cache_control");
                }
                removed += 1;
            }
        }
        to_remove -= removed;
    }
    // Remove from message content blocks.
    if to_remove > 0 {
        let mut removed = 0;
        if let Some(messages) = body.get_mut("messages").and_then(Value::as_array_mut) {
            for (mi, bi) in &message_marked {
                if removed >= to_remove {
                    break;
                }
                if let Some(blocks) = messages
                    .get_mut(*mi)
                    .and_then(|m| m.get_mut("content"))
                    .and_then(Value::as_array_mut)
                    && let Some(block) = blocks.get_mut(*bi)
                    && let Some(map) = block.as_object_mut()
                {
                    map.remove("cache_control");
                    removed += 1;
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
pub fn parse_anthropic_usage(usage: &Value) -> Usage {
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

/// XiaomiMiMo Anthropic-format response fixture.
///
/// Captured from `POST https://api.xiaomimimo.com/anthropic/v1/messages`
/// with `model=mimo-v2.5-pro`, `stream=false`, `max_tokens=1024` against
/// the Anthropic-format example in the project guide. Locks in the
/// `/anthropic/v1/messages` URL routing and the `text` + `thinking`
/// content block shape so the XiaomiMiMo provider keeps working.
pub const XIAOMIMIMO_LIVE_RESPONSE: &str = r#"{
    "id": "de877b3eb8fc400a82300513cabc1dd0",
    "type": "message",
    "role": "assistant",
    "model": "mimo-v2.5-pro",
    "stop_reason": "end_turn",
    "content": [
        {
            "type": "text",
            "text": "Hi there! I'm MiMo, a friendly AI developed by Xiaomi."
        },
        {
            "type": "thinking",
            "thinking": "The user is asking me to introduce myself.",
            "signature": ""
        }
    ],
    "usage": {"input_tokens": 55, "output_tokens": 101}
}"#;
