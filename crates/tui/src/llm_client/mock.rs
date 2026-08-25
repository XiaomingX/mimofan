//! Deterministic, offline `LlmClient` for replay-driven evaluation.
//!
//! `MockLlmClient` records canned turns (each a `Vec<StreamEvent>`) and replays
//! them one-per-`create_message_stream` call. It lets the evaluation harness
//! drive the *real* tool loop (and eventually the real `Engine`) without any
//! network or live model. Downstream code consumes the exact same
//! `StreamEvent` values a real `ApiClient` would yield, so the only difference
//! from a live run is where the bytes came from.
//!
//! The client is intentionally NOT trait-objected (`LlmClient` is not
//! `dyn`-compatible because of `impl Future` return types). It is a concrete
//! struct that *implements* `LlmClient`, mirroring `ApiClient`.

use std::collections::VecDeque;
use std::sync::Mutex;

use async_stream::stream;
use serde_json::Value;

use crate::llm_client::{LlmClient, StreamEventBox};
use crate::models::MessageRequest;
use crate::models::{ContentBlock, ContentBlockStart, MessageResponse, StreamEvent, Usage};
use anyhow::Result;

/// A queue of pre-recorded assistant turns.
///
/// Each entry is one assistant turn expressed as the SSE event stream the
/// engine would consume (e.g. `message_start`, `content_block_start` with a
/// `tool_use`, `content_block_stop`, `message_stop`).
#[derive(Debug)]
pub struct MockLlmClient {
    queue: Mutex<VecDeque<Vec<StreamEvent>>>,
    /// When true, the same front turn is replayed on every call instead of
    /// being consumed. Useful for agents that loop until a stop condition.
    repeat: bool,
}

impl MockLlmClient {
    /// Create an empty mock client.
    #[must_use]
    pub fn new() -> Self {
        Self {
            queue: Mutex::new(VecDeque::new()),
            repeat: false,
        }
    }

    /// Create a mock client that replays the same front turn forever.
    #[must_use]
    pub fn with_repeat(mut self) -> Self {
        self.repeat = true;
        self
    }

    /// Enqueue one full assistant turn (a sequence of `StreamEvent`s).
    pub fn push_message_response(&self, events: Vec<StreamEvent>) {
        self.queue
            .lock()
            .expect("mock queue poisoned")
            .push_back(events);
    }

    /// Enqueue a single text assistant turn (no tool calls).
    pub fn push_text(&self, text: &str) {
        self.push_message_response(vec![
            StreamEvent::MessageStart {
                message: minimal_message(),
            },
            StreamEvent::ContentBlockStart {
                index: 0,
                content_block: ContentBlockStart::Text {
                    text: text.to_string(),
                },
            },
            StreamEvent::ContentBlockStop { index: 0 },
            StreamEvent::MessageStop,
        ]);
    }

    /// Enqueue a single tool-call assistant turn from `(name, args_json)`.
    ///
    /// A `text` preamble is optional; when provided it is emitted as a text
    /// block before the tool-call block so multi-block turns are realistic.
    pub fn push_tool_call(&self, name: &str, args_json: Value) {
        self.push_tool_call_with_text(name, args_json, None);
    }

    /// Like [`Self::push_tool_call`] but with an optional text preamble block.
    pub fn push_tool_call_with_text(&self, name: &str, args_json: Value, preamble: Option<&str>) {
        let mut events = vec![StreamEvent::MessageStart {
            message: minimal_message(),
        }];

        let mut index = 0u32;
        if let Some(text) = preamble {
            events.push(StreamEvent::ContentBlockStart {
                index,
                content_block: ContentBlockStart::Text {
                    text: text.to_string(),
                },
            });
            events.push(StreamEvent::ContentBlockStop { index });
            index += 1;
        }

        events.push(StreamEvent::ContentBlockStart {
            index,
            content_block: ContentBlockStart::ToolUse {
                id: format!("toolu_{name}"),
                name: name.to_string(),
                input: args_json,
                caller: None,
            },
        });
        events.push(StreamEvent::ContentBlockStop { index });
        events.push(StreamEvent::MessageStop);

        self.push_message_response(events);
    }

    /// Enqueue a tool-call turn followed by a terminal "finish" text response
    /// (used when the scripted agent decides it is done after the tools run).
    pub fn push_tool_call_then_finish(&self, name: &str, args_json: Value, finish_text: &str) {
        self.push_tool_call(name, args_json);
        self.push_text(finish_text);
    }

    /// Number of turns currently queued (not yet replayed).
    #[must_use]
    pub fn pending(&self) -> usize {
        self.queue.lock().expect("mock queue poisoned").len()
    }
}

impl Default for MockLlmClient {
    fn default() -> Self {
        Self::new()
    }
}

impl LlmClient for MockLlmClient {
    fn provider_name(&self) -> &'static str {
        "mock"
    }

    fn model(&self) -> &str {
        "mock-replay"
    }

    async fn create_message(
        &self,
        _request: MessageRequest,
    ) -> Result<crate::models::MessageResponse> {
        // The harness only needs the streaming path; non-streaming is unused.
        // Build a single static message from whatever the next turn would yield.
        let front = self
            .queue
            .lock()
            .expect("mock queue poisoned")
            .front()
            .cloned()
            .unwrap_or_default();
        // Reconstruct a coarse MessageResponse from the tool-use events.
        let mut content = Vec::new();
        for event in &front {
            if let StreamEvent::ContentBlockStart {
                content_block: ContentBlockStart::Text { text },
                ..
            } = event
            {
                content.push(ContentBlock::Text {
                    text: text.clone(),
                    cache_control: None,
                });
            } else if let StreamEvent::ContentBlockStart {
                content_block:
                    ContentBlockStart::ToolUse {
                        id,
                        name,
                        input,
                        caller,
                    },
                ..
            } = event
            {
                content.push(ContentBlock::ToolUse {
                    id: id.clone(),
                    name: name.clone(),
                    input: input.clone(),
                    caller: caller.clone(),
                });
            }
        }
        Ok(MessageResponse {
            id: "msg_mock".to_string(),
            r#type: "message".to_string(),
            role: "assistant".to_string(),
            content,
            model: "mock-replay".to_string(),
            stop_reason: Some("end_turn".to_string()),
            stop_sequence: None,
            usage: Usage::default(),
            container: None,
        })
    }

    async fn create_message_stream(&self, _request: MessageRequest) -> Result<StreamEventBox> {
        let turn = {
            let mut guard = self.queue.lock().expect("mock queue poisoned");
            if self.repeat {
                guard.front().cloned().unwrap_or_default()
            } else {
                guard.pop_front().unwrap_or_default()
            }
        };

        Ok(Box::pin(stream! {
            for event in turn {
                yield Ok(event);
            }
        }))
    }
}

/// Build a minimal `MessageResponse` suitable for `message_start` events.
fn minimal_message() -> MessageResponse {
    MessageResponse {
        id: "msg_mock".to_string(),
        r#type: "message".to_string(),
        role: "assistant".to_string(),
        content: Vec::new(),
        model: "mock-replay".to_string(),
        stop_reason: Some("tool_use".to_string()),
        stop_sequence: None,
        usage: Usage::default(),
        container: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt;

    #[tokio::test]
    async fn replays_canned_turn_text_toolcall_finish() {
        let mock = MockLlmClient::new();
        mock.push_message_response(vec![
            StreamEvent::MessageStart {
                message: minimal_message(),
            },
            StreamEvent::ContentBlockStart {
                index: 0,
                content_block: ContentBlockStart::Text {
                    text: "thinking…".to_string(),
                },
            },
            StreamEvent::ContentBlockStop { index: 0 },
            StreamEvent::ContentBlockStart {
                index: 1,
                content_block: ContentBlockStart::ToolUse {
                    id: "toolu_abc".to_string(),
                    name: "gadget_chain_trace".to_string(),
                    input: serde_json::json!({ "sink": "x", "present_gadgets": [] }),
                    caller: None,
                },
            },
            StreamEvent::ContentBlockStop { index: 1 },
            StreamEvent::MessageStop,
        ]);

        let mut stream = mock
            .create_message_stream(MessageRequest {
                model: "mock".to_string(),
                messages: Vec::new(),
                max_tokens: 1024,
                system: None,
                tools: None,
                tool_choice: None,
                metadata: None,
                thinking: None,
                reasoning_effort: None,
                stream: Some(true),
                temperature: None,
                top_p: None,
                response_format: None,
            })
            .await
            .unwrap();

        // First event: message_start
        let first = stream.next().await.unwrap().unwrap();
        assert!(matches!(first, StreamEvent::MessageStart { .. }));

        // Find the tool_use event and assert name + args round-trip.
        let mut found = false;
        while let Some(item) = stream.next().await {
            let event = item.unwrap();
            if let StreamEvent::ContentBlockStart {
                content_block: ContentBlockStart::ToolUse { name, input, .. },
                ..
            } = event
            {
                assert_eq!(name, "gadget_chain_trace");
                assert_eq!(input["sink"], "x");
                found = true;
                break;
            }
        }
        assert!(found, "expected a tool_use event to be replayed");

        // Queue should now be empty (not repeat mode).
        assert_eq!(mock.pending(), 0);
    }

    #[tokio::test]
    async fn helper_push_tool_call_replays() {
        let mock = MockLlmClient::new();
        mock.push_tool_call(
            "run_poc",
            serde_json::json!({ "command": "echo hi", "expect": "hi" }),
        );

        let mut stream = mock
            .create_message_stream(MessageRequest {
                model: "mock".to_string(),
                messages: Vec::new(),
                max_tokens: 1024,
                system: None,
                tools: None,
                tool_choice: None,
                metadata: None,
                thinking: None,
                reasoning_effort: None,
                stream: Some(true),
                temperature: None,
                top_p: None,
                response_format: None,
            })
            .await
            .unwrap();
        let mut captured: Option<(String, Value)> = None;
        while let Some(item) = stream.next().await {
            if let StreamEvent::ContentBlockStart {
                content_block: ContentBlockStart::ToolUse { name, input, .. },
                ..
            } = item.unwrap()
            {
                captured = Some((name, input));
            }
        }
        let (name, input) = captured.expect("tool_use event present");
        assert_eq!(name, "run_poc");
        assert_eq!(input["command"], "echo hi");
    }
}
