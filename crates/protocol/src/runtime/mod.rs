use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const RUNTIME_EVENT_ENVELOPE_SCHEMA_VERSION: u32 = 1;
pub const RUNTIME_API_VERSION: &str = "1.0";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeEventEnvelope {
    #[serde(default = "default_runtime_event_envelope_schema_version")]
    pub schema_version: u32,
    pub seq: u64,
    pub event: String,
    pub kind: String,
    pub thread_id: String,
    pub turn_id: Option<String>,
    pub item_id: Option<String>,
    pub timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    pub payload: Value,
    #[serde(default)]
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

fn default_runtime_event_envelope_schema_version() -> u32 {
    RUNTIME_EVENT_ENVELOPE_SCHEMA_VERSION
}

// ---------------------------------------------------------------------------
// Capability advertisement
// ---------------------------------------------------------------------------

/// Fixed capability map advertised by `GET /v1/runtime/info`.
///
/// All fields are required on serialization so clients can rely on the shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeCapabilities {
    pub threads: bool,
    pub turns: bool,
    pub turn_steer: bool,
    pub turn_interrupt: bool,
    pub event_replay: bool,
    pub external_tools: bool,
    pub environments: bool,
    pub worker_runtime: bool,
}

/// Experimental opt-in flags advertised by `GET /v1/runtime/info`.
///
/// Fields are additive and default to `false` when omitted by older servers.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RuntimeExperimentalCapabilities {
    #[serde(default)]
    pub environments: bool,
}

// ---------------------------------------------------------------------------
// External Tool Bridge protocol types
// ---------------------------------------------------------------------------

/// Specification for a dynamic external tool registered by a runtime client.
///
/// Example JSON from the spec:
///
/// ```json
/// {
///   "namespace": "tau_bench",
///   "name": "get_reservation",
///   "description": "Look up an airline reservation.",
///   "input_schema": {
///     "type": "object",
///     "properties": {
///       "reservation_id": { "type": "string" }
///     },
///     "required": ["reservation_id"],
///     "additionalProperties": false
///   }
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DynamicToolSpec {
    /// Optional namespace that groups related tools (e.g. `"tau_bench"`).
    /// When present, the runtime may expose the tool as
    /// `<namespace>::<name>` to the model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,

    /// Short tool name. Combined with `namespace` it forms a unique tool id.
    pub name: String,

    /// Human-readable description exposed to the model.
    pub description: String,

    /// JSON Schema describing the tool's input parameters.
    pub input_schema: Value,

    /// If true, the runtime may defer schema validation / tool loading until
    /// the model actually calls the tool.
    ///
    /// Defaults to `false` so that older clients omitting this field still
    /// behave the same way.
    #[serde(default)]
    pub defer_loading: bool,
}

/// Lifecycle status of a dynamic tool item shown in thread detail and event
/// payloads.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DynamicToolItemStatus {
    InProgress,
    Completed,
    Failed,
}

/// Parameters identifying a dynamic tool call request emitted by the runtime.
///
/// This is the typed payload for `tool_call.requested` events and also the
/// natural identifier used when the runtime looks up a pending call.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DynamicToolCallParams {
    pub thread_id: String,
    pub turn_id: String,
    pub call_id: String,

    /// Optional namespace that was registered with the tool.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,

    /// Tool name that the model invoked.
    pub tool: String,

    /// Arguments supplied by the model, validated against `input_schema`.
    pub arguments: Value,
}

/// Result submitted by a runtime client after executing a dynamic tool.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DynamicToolCallResult {
    /// Whether the client-side tool execution succeeded.
    pub success: bool,

    /// Content fragments returned by the tool.
    ///
    /// Defaults to an empty vector when omitted so clients can send a minimal
    /// `{ "success": false }` payload.
    #[serde(default)]
    pub content: Vec<DynamicToolCallContent>,
}

/// A single content fragment inside a [`DynamicToolCallResult`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DynamicToolCallContent {
    InputText { text: String },
    InputImage { image_url: String },
}

// ---------------------------------------------------------------------------
// Environment targeting protocol types
// ---------------------------------------------------------------------------

/// Environment target selected for a turn's shell/filesystem work.
///
/// Example JSON:
///
/// ```json
/// {
///   "environment_id": "local",
///   "cwd": "/workspace"
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TurnEnvironmentParams {
    pub environment_id: String,
    pub cwd: PathBuf,
}

