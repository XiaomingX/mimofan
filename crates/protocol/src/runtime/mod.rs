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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn dynamic_tool_spec_roundtrip() {
        let spec = DynamicToolSpec {
            namespace: Some("tau_bench".into()),
            name: "get_reservation".into(),
            description: "Look up an airline reservation.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "reservation_id": { "type": "string" }
                },
                "required": ["reservation_id"],
                "additionalProperties": false
            }),
            defer_loading: false,
        };

        let serialized = serde_json::to_string(&spec).expect("serialize dynamic tool spec");
        let deserialized: DynamicToolSpec = serde_json::from_str(&serialized).expect("deserialize dynamic tool spec");
        assert_eq!(spec, deserialized);
    }

    #[test]
    fn dynamic_tool_spec_omits_defer_loading_defaults_false() {
        let json = r#"{
            "namespace": "tau_bench",
            "name": "get_reservation",
            "description": "Look up an airline reservation.",
            "input_schema": { "type": "object" }
        }"#;

        let spec: DynamicToolSpec = serde_json::from_str(json).expect("deserialize dynamic tool spec omitting defer_loading");
        assert_eq!(spec.namespace, Some("tau_bench".into()));
        assert_eq!(spec.name, "get_reservation");
        assert!(!spec.defer_loading);
    }

    #[test]
    fn dynamic_tool_item_status_snake_case() {
        assert_eq!(
            serde_json::to_string(&DynamicToolItemStatus::InProgress).expect("serialize in_progress status"),
            "\"in_progress\""
        );
        assert_eq!(
            serde_json::from_str::<DynamicToolItemStatus>("\"completed\"").expect("deserialize completed status"),
            DynamicToolItemStatus::Completed
        );
        assert_eq!(
            serde_json::from_str::<DynamicToolItemStatus>("\"failed\"").expect("deserialize failed status"),
            DynamicToolItemStatus::Failed
        );
    }

    #[test]
    fn dynamic_tool_call_params_roundtrip() {
        let params = DynamicToolCallParams {
            thread_id: "thr_123".into(),
            turn_id: "turn_456".into(),
            call_id: "call_abc".into(),
            namespace: Some("tau_bench".into()),
            tool: "get_reservation".into(),
            arguments: json!({ "reservation_id": "ABC123" }),
        };

        let serialized = serde_json::to_string(&params).expect("serialize dynamic tool call params");
        let deserialized: DynamicToolCallParams = serde_json::from_str(&serialized).expect("deserialize dynamic tool call params");
        assert_eq!(params, deserialized);
    }

    #[test]
    fn dynamic_tool_call_content_roundtrip() {
        let content = vec![
            DynamicToolCallContent::InputText {
                text: "{\"status\":\"confirmed\"}".into(),
            },
            DynamicToolCallContent::InputImage {
                image_url: "http://example.com/receipt.png".into(),
            },
        ];

        let value = serde_json::to_value(&content).expect("serialize dynamic tool call content");
        let deserialized: Vec<DynamicToolCallContent> = serde_json::from_value(value).expect("deserialize dynamic tool call content");
        assert_eq!(content, deserialized);

        // Verify the exact JSON tag names expected by the spec.
        assert_eq!(
            serde_json::to_string(&DynamicToolCallContent::InputText { text: "x".into() }).expect("serialize input_text content"),
            r#"{"type":"input_text","text":"x"}"#
        );
        assert_eq!(
            serde_json::to_string(&DynamicToolCallContent::InputImage {
                image_url: "y".into()
            })
            .expect("serialize input_image content"),
            r#"{"type":"input_image","image_url":"y"}"#
        );
    }

    #[test]
    fn dynamic_tool_call_result_defaults_empty_content() {
        let json = r#"{ "success": false }"#;
        let result: DynamicToolCallResult = serde_json::from_str(json).expect("deserialize dynamic tool call result defaults");
        assert!(!result.success);
        assert!(result.content.is_empty());
    }

    #[test]
    fn dynamic_tool_call_result_roundtrip_with_content() {
        let result = DynamicToolCallResult {
            success: true,
            content: vec![DynamicToolCallContent::InputText {
                text: "done".into(),
            }],
        };

        let serialized = serde_json::to_string(&result).expect("serialize dynamic tool call result");
        let deserialized: DynamicToolCallResult = serde_json::from_str(&serialized).expect("deserialize dynamic tool call result");
        assert_eq!(result, deserialized);
    }

    #[test]
    fn turn_environment_params_roundtrip() {
        let env = TurnEnvironmentParams {
            environment_id: "local".into(),
            cwd: PathBuf::from("/workspace"),
        };

        let serialized = serde_json::to_string(&env).expect("serialize turn environment params");
        let deserialized: TurnEnvironmentParams = serde_json::from_str(&serialized).expect("deserialize turn environment params");
        assert_eq!(env, deserialized);

        // Verify JSON from the spec deserializes directly.
        let from_spec = r#"{
            "environment_id": "local",
            "cwd": "/workspace"
        }"#;
        let parsed: TurnEnvironmentParams = serde_json::from_str(from_spec).expect("deserialize turn environment params from spec");
        assert_eq!(parsed.environment_id, "local");
        assert_eq!(parsed.cwd, PathBuf::from("/workspace"));
    }

    #[test]
    fn runtime_capabilities_serializes_expected_shape() {
        let caps = RuntimeCapabilities {
            threads: true,
            turns: true,
            turn_steer: true,
            turn_interrupt: true,
            event_replay: true,
            external_tools: false,
            environments: false,
            worker_runtime: false,
        };
        let value = serde_json::to_value(&caps).expect("serialize runtime capabilities");
        let obj = value.as_object().expect("runtime capabilities as object");
        assert_eq!(obj.get("threads").expect("threads key present"), &json!(true));
        assert_eq!(obj.get("external_tools").expect("external_tools key present"), &json!(false));
        assert!(obj.contains_key("worker_runtime"));
    }

    #[test]
    fn runtime_event_envelope_schema_version_default() {
        let json = r#"{
            "seq": 1,
            "event": "test",
            "kind": "test",
            "thread_id": "thr_1",
            "timestamp": "2026-06-12T00:00:00Z",
            "payload": {}
        }"#;
        let envelope: RuntimeEventEnvelope = serde_json::from_str(json).expect("deserialize runtime event envelope");
        assert_eq!(
            envelope.schema_version,
            RUNTIME_EVENT_ENVELOPE_SCHEMA_VERSION
        );
    }
}
