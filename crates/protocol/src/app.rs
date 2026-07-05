use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{EventFrame, UserInputAnswerEvent};

/// Application-level requests that are not tied to a specific thread.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AppRequest {
    /// Query the server's capabilities.
    Capabilities,
    /// Read a configuration value by key.
    ConfigGet { key: String },
    /// Set a configuration key to a value.
    ConfigSet { key: String, value: String },
    /// Remove a configuration key.
    ConfigUnset { key: String },
    /// List all configuration entries.
    ConfigList,
    /// List available models.
    Models,
    /// List threads that are currently loaded in memory.
    ThreadLoadedList,
    /// Submit answers to a prior [`EventFrame::UserInputRequest`].
    ///
    /// `request_id` must match a pending clarification request. Headless
    /// clients use this to return the user's selections back to the runtime.
    SubmitUserInput {
        request_id: String,
        answers: Vec<UserInputAnswerEvent>,
    },
}

/// Response to an [`AppRequest`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppResponse {
    /// Whether the request succeeded.
    pub ok: bool,
    /// The response payload.
    pub data: Value,
    /// Streaming events associated with this response.
    #[serde(default)]
    pub events: Vec<EventFrame>,
}

/// A simple prompt request that sends text to the model and returns output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptRequest {
    /// Optional thread context for the prompt.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    /// The prompt text.
    pub prompt: String,
    /// Model override, or the default if omitted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// OpenAI-compatible `response_format` (e.g. `{"type":"json_object"}`).
    ///
    /// Forwarded to the upstream Chat Completions endpoint when set. The
    /// Anthropic Messages dialect ignores this field by design; clients
    /// targeting `…/anthropic` should rely on prompt-level JSON instructions
    /// instead. Schema is exposed before the underlying runtime pipeline
    /// wires it through, so CLI / app-server callers can already opt in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<Value>,
}

/// Response to a [`PromptRequest`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptResponse {
    /// The model's output text.
    pub output: String,
    /// The model that produced the output.
    pub model: String,
    /// Streaming events associated with this response.
    #[serde(default)]
    pub events: Vec<EventFrame>,
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
