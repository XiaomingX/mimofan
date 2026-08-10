//! Native Google Gemini (`generativelanguage`) adapter (#737).
//!
//! mimofan's internal wire types are Anthropic-shaped, so this adapter
//! translates *out* to the Gemini `generateContent` dialect:
//!
//! - request shaping: `MessageRequest` → Gemini `contents` / `systemInstruction`
//!   / `generationConfig` / `tools[].functionDeclarations`;
//! - response normalization: Gemini `candidates[].content.parts[].text` and
//!   `usageMetadata` are folded back into mimofan's `MessageResponse`;
//! - SSE pass-through: the `:streamGenerateContent?alt=sse` endpoint emits
//!   `data: {...}` frames carrying incremental `candidates` deltas.
//!
//! ## Scope of the first cut
//!
//! This adapter covers **text generation + streaming + usage normalization**.
//! It deliberately does *not* yet implement the function-calling round-trip
//! (`functionCall` / `functionResponse` ↔ mimofan's `tool_use` blocks), nor
//! thinking/reasoning-content mapping or `safetyRatings` handling. Those are
//! tracked as follow-up work; the `functionDeclarations` writer is kept so the
//! tool *surface* is visible to Gemini, but tool *results* are not yet
//! consumed. See issue #737.
//!
//! Modeled on `client/anthropic.rs` (one file per dialect, no protocol hacks
//! in the shared paths).

use anyhow::{Context, Result};
use serde_json::{Value, json};

use crate::llm_client::StreamEventBox;
use crate::logging;
use crate::models::{
    ContentBlock, Delta, Message, MessageRequest, MessageResponse, StreamEvent, SystemPrompt,
    Usage,
};

use super::{ApiClient, ERROR_BODY_MAX_BYTES, bounded_error_text};

/// Build the Gemini `generateContent` / `streamGenerateContent` URL.
///
/// The configured `base_url` already carries the API version (e.g.
/// `…/v1beta`), so we append the model-scoped method directly. Gemini
/// authenticates with the `key` query parameter against the public
/// `generativelanguage.googleapis.com` endpoint, so it is appended here
/// (the shared `Authorization: Bearer` header is left in place but ignored by
/// Google's endpoint).
pub fn gemini_generate_content_url(
    base_url: &str,
    model: &str,
    stream: bool,
    api_key: &str,
) -> String {
    let trimmed = base_url.trim_end_matches('/');
    let method = if stream {
        "streamGenerateContent?alt=sse"
    } else {
        "generateContent"
    };
    let key_suffix = if api_key.is_empty() {
        String::new()
    } else if stream {
        format!("&key={api_key}")
    } else {
        format!("?key={api_key}")
    };
    format!("{trimmed}/models/{model}:{method}{key_suffix}")
}

/// Convert one internal message to a Gemini `content` value, or `None` when no
/// text parts survive (Gemini rejects empty `parts`).
fn message_to_gemini(message: &Message) -> Option<Value> {
    let role = match message.role.as_str() {
        "user" => "user",
        "assistant" => "model",
        // Gemini only understands user/model; map anything else onto the
        // closest participant rather than dropping the turn silently.
        "system" => "user",
        _ => "user",
    };
    let parts: Vec<Value> = message
        .content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text, .. } => Some(json!({ "text": text })),
            // First cut: only text is carried. Thinking / tool_use / tool_result
            // blocks are skipped (see module docs); they are surfaced once the
            // function-calling round-trip lands.
            _ => None,
        })
        .collect();
    if parts.is_empty() {
        return None;
    }
    Some(json!({ "role": role, "parts": parts }))
}

/// Extract plain text from a `SystemPrompt` (merging structured blocks).
fn system_prompt_text(system: &SystemPrompt) -> String {
    match system {
        SystemPrompt::Text(text) => text.clone(),
        SystemPrompt::Blocks(blocks) => blocks
            .iter()
            .map(|b| b.text.clone())
            .collect::<Vec<_>>()
            .join("\n\n---\n\n"),
    }
}

impl ApiClient {
/// Build the native `generateContent` request body from a [`MessageRequest`].
///
/// Free function (not tied to `ApiClient`) so it can be unit-tested without a
/// live client; `handle_gemini_*` call it with the request in scope.
pub(super) fn build_gemini_body(request: &MessageRequest, _stream: bool) -> Value {
        let mut body = json!({});

        if let Some(system) = request.system.as_ref() {
            let text = system_prompt_text(system);
            if !text.trim().is_empty() {
                body["systemInstruction"] = json!({ "parts": [{ "text": text }] });
            }
        }

        let contents: Vec<Value> = request
            .messages
            .iter()
            .filter_map(message_to_gemini)
            .collect();
        body["contents"] = json!(contents);

        let mut gen_config = json!({ "maxOutputTokens": request.max_tokens });
        if let Some(temperature) = request.temperature {
            gen_config["temperature"] = json!(temperature);
        }
        if let Some(top_p) = request.top_p {
            gen_config["topP"] = json!(top_p);
        }
        body["generationConfig"] = gen_config;

        // First cut: declare tools so Gemini exposes the function surface, but
        // we do not yet consume `functionCall` results. See module docs.
        if let Some(tools) = request.tools.as_ref()
            && !tools.is_empty()
        {
            body["tools"] = json!([{
                "functionDeclarations": tools
                    .iter()
                    .map(|tool| json!({
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": tool.input_schema,
                    }))
                    .collect::<Vec<_>>()
            }]);
        }

        body
    }

    async fn send_gemini_request(&self, body: &Value, stream: bool) -> Result<reqwest::Response> {
        let url = gemini_generate_content_url(&self.base_url, &body["model"].as_str().unwrap_or(""), stream, self.api_key());
        self.wait_for_rate_limit().await;
        let response = self
            .http_client
            .post(&url)
            .json(body)
            .send()
            .await
            .context("Gemini generateContent request failed")?;

        let status = response.status();
        if !status.is_success() {
            let raw = bounded_error_text(response, ERROR_BODY_MAX_BYTES).await;
            self.mark_request_failure(&format!("gemini status={status}"))
                .await;
            anyhow::bail!("Gemini API error (HTTP {status}): {raw}");
        }
        self.mark_request_success().await;
        Ok(response)
    }

    /// Normalize a Gemini `finishReason` into mimofan's `stop_reason`.
pub(super) fn gemini_stop_reason(finish_reason: Option<&str>) -> Option<String> {
    match finish_reason {
        Some("STOP") => Some("end_turn".to_string()),
        Some("MAX_TOKENS") => Some("max_tokens".to_string()),
        Some("SAFETY" | "RECITATION" | "OTHER") => Some("content_filter".to_string()),
        _ => None,
    }
}

/// Fold a Gemini `generateContent` response value into a [`MessageResponse`].
pub(super) fn parse_gemini_response(value: Value, model: &str) -> Result<MessageResponse> {
        let candidate = value
            .get("candidates")
            .and_then(Value::as_array)
            .and_then(|c| c.first())
            .context("Gemini response missing candidates[0]")?;

        let mut text = String::new();
        if let Some(parts) = candidate
            .get("content")
            .and_then(|c| c.get("parts"))
            .and_then(Value::as_array)
        {
            for part in parts {
                if let Some(t) = part.get("text").and_then(Value::as_str) {
                    text.push_str(t);
                }
            }
        }
        let content = if text.is_empty() {
            Vec::new()
        } else {
            vec![ContentBlock::Text {
                text,
                cache_control: None,
            }]
        };

        let stop_reason = Self::gemini_stop_reason(
            candidate.get("finishReason").and_then(Value::as_str),
        );

        let mut usage = Usage {
            input_tokens: 0,
            output_tokens: 0,
            ..Default::default()
        };
        if let Some(meta) = value.get("usageMetadata") {
            usage.input_tokens = meta
                .get("promptTokenCount")
                .and_then(Value::as_u64)
                .unwrap_or(0) as u32;
            usage.output_tokens = meta
                .get("candidatesTokenCount")
                .and_then(Value::as_u64)
                .unwrap_or(0) as u32;
            usage.prompt_cache_hit_tokens = meta
                .get("cachedContentTokenCount")
                .and_then(Value::as_u64)
                .map(|v| v as u32);
        }

        Ok(MessageResponse {
            id: value
                .get("responseId")
                .and_then(Value::as_str)
                .unwrap_or("gemini")
                .to_string(),
            r#type: "message".to_string(),
            role: "assistant".to_string(),
            content,
            model: model.to_string(),
            stop_reason,
            stop_sequence: None,
            container: None,
            usage,
        })
    }

/// Extract the incremental text delta from one Gemini SSE `candidates` frame.
pub(super) fn gemini_delta_text(value: &Value) -> String {
    let mut text = String::new();
    if let Some(candidate) = value
        .get("candidates")
        .and_then(Value::as_array)
        .and_then(|c| c.first())
        && let Some(parts) = candidate.get("content").and_then(|c| c.get("parts")).and_then(Value::as_array)
    {
        for part in parts {
            if let Some(t) = part.get("text").and_then(Value::as_str) {
                text.push_str(t);
            }
        }
    }
    text
}

/// Whether a Gemini SSE frame carries a `finishReason` (signals end of stream).
pub(super) fn gemini_frame_finished(value: &Value) -> bool {
    value
        .get("candidates")
        .and_then(Value::as_array)
        .and_then(|c| c.first())
        .and_then(|c| c.get("finishReason"))
        .is_some()
}

    /// Handle a non-streaming `generateContent` request.
    pub(super) async fn handle_gemini_message(
        &self,
        request: MessageRequest,
    ) -> Result<MessageResponse> {
        let model = request.model.clone();
        let body = Self::build_gemini_body(&request, false);
        let response = self.send_gemini_request(&body, false).await?;
        let value: Value = response
            .json()
            .await
            .context("Failed to parse Gemini generateContent response")?;
        Self::parse_gemini_response(value, &model)
    }

    /// Handle a streaming `streamGenerateContent?alt=sse` request.
    pub(super) async fn handle_gemini_stream(
        &self,
        request: MessageRequest,
    ) -> Result<StreamEventBox> {
        let model = request.model.clone();
        let body = Self::build_gemini_body(&request, true);
        let response = self.send_gemini_request(&body, true).await?;

        let stream_idle_timeout = self.stream_idle_timeout;
        let byte_stream = response.bytes_stream();

        let stream = async_stream::stream! {
            use futures_util::StreamExt;

            let mut buffer = String::new();
            tokio::pin!(byte_stream);

            // Track whether we've emitted the leading message_start /
            // content_block_start pair, so deltas arrive under a real block.
            let mut started = false;
            let mut buffer_text = String::new();

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

                    let Some(data) = super::extract_sse_data_value(&line) else {
                        continue;
                    };

                    match serde_json::from_str::<Value>(data) {
                        Ok(value) => {
                            if let Some(error) = value.get("error") {
                                let message = error
                                    .get("message")
                                    .and_then(Value::as_str)
                                    .unwrap_or("unknown Gemini stream error")
                                    .to_string();
                                yield Err(anyhow::anyhow!("Gemini stream error: {message}"));
                                return;
                            }

                            let delta_text = Self::gemini_delta_text(&value);

                            if !started {
                                started = true;
                                yield Ok(StreamEvent::MessageStart {
                                    message: MessageResponse {
                                        id: value.get("responseId").and_then(Value::as_str).unwrap_or("gemini").to_string(),
                                        r#type: "message".to_string(),
                                        role: "assistant".to_string(),
                                        content: Vec::new(),
                                        model: model.clone(),
                                        stop_reason: None,
                                        stop_sequence: None,
                                        container: None,
                                        usage: Usage::default(),
                                    },
                                });
                                yield Ok(StreamEvent::ContentBlockStart {
                                    index: 0,
                                    content_block: crate::models::ContentBlockStart::Text {
                                        text: String::new(),
                                    },
                                });
                            }

                            if !delta_text.is_empty() {
                                buffer_text.push_str(&delta_text);
                                yield Ok(StreamEvent::ContentBlockDelta {
                                    index: 0,
                                    delta: Delta::TextDelta { text: delta_text },
                                });
                            }

                            // Gemini streams a final chunk with `finishReason`
                            // (or simply ends). Emit MessageStop once we've seen
                            // a finishReason or the stream ends.
                            let finished = Self::gemini_frame_finished(&value);
                            if finished {
                                yield Ok(StreamEvent::ContentBlockStop { index: 0 });
                                yield Ok(StreamEvent::MessageStop);
                                return;
                            }
                        }
                        Err(e) => {
                            logging::warn(format!("Failed to parse Gemini SSE event: {e}"));
                        }
                    }
                }
            }

            // Stream ended without an explicit finishReason frame.
            if started {
                yield Ok(StreamEvent::ContentBlockStop { index: 0 });
                yield Ok(StreamEvent::MessageStop);
            }
        };

        Ok(Box::pin(stream))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ContentBlock, Message, SystemPrompt, Tool};

    fn text_message(role: &str, text: &str) -> Message {
        Message {
            role: role.to_string(),
            content: vec![ContentBlock::Text {
                text: text.to_string(),
                cache_control: None,
            }],
        }
    }

    #[test]
    fn url_non_streaming_appends_key() {
        let url = gemini_generate_content_url(
            "https://generativelanguage.googleapis.com/v1beta",
            "gemini-2.0-flash",
            false,
            "ABC123",
        );
        assert_eq!(
            url,
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.0-flash:generateContent?key=ABC123"
        );
    }

    #[test]
    fn url_streaming_uses_alt_sse_and_key() {
        let url = gemini_generate_content_url(
            "https://generativelanguage.googleapis.com/v1beta/",
            "gemini-2.0-flash",
            true,
            "ABC123",
        );
        assert_eq!(
            url,
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.0-flash:streamGenerateContent?alt=sse&key=ABC123"
        );
    }

    #[test]
    fn url_omits_key_when_empty() {
        let url = gemini_generate_content_url(
            "https://example.com/v1beta",
            "m",
            false,
            "",
        );
        assert_eq!(url, "https://example.com/v1beta/models/m:generateContent");
    }

    #[test]
    fn body_maps_roles_and_parts() {
        let request = MessageRequest {
            model: "gemini-2.0-flash".to_string(),
            messages: vec![
                text_message("user", "hello"),
                text_message("assistant", "hi there"),
            ],
            max_tokens: 512,
            system: Some(SystemPrompt::Text("be terse".to_string())),
            tools: None,
            tool_choice: None,
            metadata: None,
            thinking: None,
            reasoning_effort: None,
            stream: None,
            temperature: Some(0.7),
            top_p: Some(0.9),
            response_format: None,
        };
        let body = ApiClient::build_gemini_body(&request, false);

        let contents = body["contents"].as_array().unwrap();
        assert_eq!(contents.len(), 2);
        assert_eq!(contents[0]["role"], "user");
        assert_eq!(contents[0]["parts"][0]["text"], "hello");
        assert_eq!(contents[1]["role"], "model");
        assert_eq!(contents[1]["parts"][0]["text"], "hi there");

        // systemInstruction is top-level and NOT inside contents.
        assert_eq!(body["systemInstruction"]["parts"][0]["text"], "be terse");
        assert!(contents
            .iter()
            .all(|c| c.get("parts").map_or(true, |p| p
                .as_array()
                .map_or(true, |a| a.iter().all(|part| part.get("text").map_or(
                    false,
                    |t| t.as_str().map_or(true, |s| s != "be terse")
                ))))));

        // generationConfig carries maxOutputTokens from max_tokens.
        assert_eq!(body["generationConfig"]["maxOutputTokens"], 512);
        assert_eq!(body["generationConfig"]["temperature"], 0.7);
        assert_eq!(body["generationConfig"]["topP"], 0.9);
    }

    #[test]
    fn body_skips_empty_messages() {
        let request = MessageRequest {
            model: "m".to_string(),
            messages: vec![text_message("user", "real"), Message {
                role: "user".to_string(),
                content: vec![], // would produce no text parts → dropped
            }],
            max_tokens: 10,
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
        let body = ApiClient::build_gemini_body(&request, false);
        assert_eq!(body["contents"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn body_declares_function_when_tools_present() {
        let tool = Tool {
            tool_type: Some("function".to_string()),
            name: "read_file".to_string(),
            description: "read a file".to_string(),
            input_schema: json!({ "type": "object" }),
            allowed_callers: None,
            defer_loading: None,
            input_examples: None,
            strict: None,
            cache_control: None,
        };
        let request = MessageRequest {
            model: "m".to_string(),
            messages: vec![text_message("user", "go")],
            max_tokens: 10,
            system: None,
            tools: Some(vec![tool]),
            tool_choice: None,
            metadata: None,
            thinking: None,
            reasoning_effort: None,
            stream: None,
            temperature: None,
            top_p: None,
            response_format: None,
        };
        let body = ApiClient::build_gemini_body(&request, false);
        let decls = body["tools"][0]["functionDeclarations"].as_array().unwrap();
        assert_eq!(decls.len(), 1);
        assert_eq!(decls[0]["name"], "read_file");
    }

    #[test]
    fn parse_response_folds_text_usage_and_stop() {
        let value = json!({
            "responseId": "resp-1",
            "candidates": [{
                "content": { "parts": [{ "text": "hello " }, { "text": "world" }] },
                "finishReason": "STOP"
            }],
            "usageMetadata": {
                "promptTokenCount": 11,
                "candidatesTokenCount": 22,
                "cachedContentTokenCount": 5
            }
        });
        let resp = ApiClient::parse_gemini_response(value, "gemini-2.0-flash").unwrap();
        assert_eq!(resp.content.len(), 1);
        match &resp.content[0] {
            ContentBlock::Text { text, .. } => assert_eq!(text, "hello world"),
            _ => panic!("expected text block"),
        }
        assert_eq!(resp.stop_reason.as_deref(), Some("end_turn"));
        assert_eq!(resp.usage.input_tokens, 11);
        assert_eq!(resp.usage.output_tokens, 22);
        assert_eq!(resp.usage.prompt_cache_hit_tokens, Some(5));
    }

    #[test]
    fn parse_response_maps_max_tokens_stop() {
        let value = json!({
            "candidates": [{ "finishReason": "MAX_TOKENS" }],
            "usageMetadata": {}
        });
        let resp = ApiClient::parse_gemini_response(value, "m").unwrap();
        assert_eq!(resp.stop_reason.as_deref(), Some("max_tokens"));
    }

    #[test]
    fn delta_text_and_finished_flag() {
        let frame = json!({
            "candidates": [{
                "content": { "parts": [{ "text": "par" }, { "text": "tial" }] }
            }]
        });
        assert_eq!(ApiClient::gemini_delta_text(&frame), "partial");
        assert!(!ApiClient::gemini_frame_finished(&frame));

        let end = json!({ "candidates": [{ "finishReason": "STOP" }] });
        assert!(ApiClient::gemini_frame_finished(&end));
    }
}

