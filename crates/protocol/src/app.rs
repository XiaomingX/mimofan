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
