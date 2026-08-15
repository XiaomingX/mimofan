//! Synthetic terminator tool for headless `--json-schema` runs (issue #824).
//!
//! Unlike the general-purpose `synthetic_output` tool (#729) — which is an
//! *optional* capability tool that returns validated JSON to the model — this
//! tool is an **end-state** mechanism: the model must call it exactly once to
//! submit its final result. A schema-valid submission sets a shared flag that
//! the `exec` loop observes to terminate the run and emit the submitted data
//! as the final result.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use jsonschema::Validator;
use serde_json::{Value, json};

use crate::tools::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec,
};

/// Tool name the headless run registers and the exec loop watches for.
pub const JSON_SCHEMA_TERMINATOR_NAME: &str = "json_schema_terminate";

/// Metadata key on a successful terminator result. The exec loop inspects
/// `ToolResult.metadata[TERMINATE_MARKER] == true` to know the run should end.
pub const TERMINATE_MARKER: &str = "terminate_run";

/// Shared sink the terminator writes the accepted submission into. The exec
/// loop owns the `Arc<Mutex<…>>` clone and reads it once the terminator fires.
pub type SubmissionSlot = Arc<Mutex<Option<Value>>>;

/// Parse a `--json-schema` CLI value, which may be either an inline JSON string
/// or a path to a file containing JSON. Returns the parsed schema object.
pub fn parse_json_schema_arg(raw: &str) -> Result<Value, String> {
    let trimmed = raw.trim();
    // Heuristic: a value that starts with `{` (or whitespace + `{`) is inline
    // JSON; otherwise treat it as a filesystem path.
    if trimmed.starts_with('{') {
        serde_json::from_str::<Value>(trimmed)
            .map_err(|e| format!("inline --json-schema is not valid JSON: {e}"))
    } else {
        let text = std::fs::read_to_string(raw)
            .map_err(|e| format!("could not read --json-schema file '{raw}': {e}"))?;
        serde_json::from_str::<Value>(&text)
            .map_err(|e| format!("--json-schema file '{raw}' is not valid JSON: {e}"))
    }
}

/// Synthetic terminator tool. On a schema-valid submission it records the
/// payload in `submission` and returns a success result tagged with
/// [`TERMINATE_MARKER`]; on an invalid submission it returns a readable error
/// and the run continues.
pub struct JsonSchemaTerminator {
    schema: Value,
    submission: SubmissionSlot,
}

impl JsonSchemaTerminator {
    /// Build a terminator bound to `schema` and a shared submission slot.
    pub fn new(schema: Value, submission: SubmissionSlot) -> Self {
        Self { schema, submission }
    }

    /// Validate `value` against the configured schema using the full
    /// `jsonschema` engine. Returns the first error as a readable string, or
    /// `Ok(())` when conformant.
    pub fn validate(&self, value: &Value) -> Result<(), String> {
        let validator = Validator::new(&self.schema)
            .map_err(|e| format!("invalid --json-schema (could not build validator): {e}"))?;
        let errors: Vec<String> = validator
            .iter_errors(value)
            .map(|e| e.to_string())
            .collect();
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }
}

#[async_trait]
impl ToolSpec for JsonSchemaTerminator {
    fn name(&self) -> &'static str {
        JSON_SCHEMA_TERMINATOR_NAME
    }

    fn description(&self) -> &'static str {
        "Submit the final result for this run as a JSON object. The object MUST conform to the schema supplied via `--json-schema`. A valid submission ends the run and becomes the final output; an invalid submission returns the schema errors so you can correct and resubmit."
    }

    fn input_schema(&self) -> Value {
        // The terminator accepts exactly the schema it validates against, so
        // the model sees the contract it must satisfy.
        let mut data_props = self
            .schema
            .as_object()
            .cloned()
            .unwrap_or_else(serde_json::Map::new);
        data_props.insert(
            "description".to_string(),
            json!("The final result object, conforming to the --json-schema contract."),
        );
        let mut properties = serde_json::Map::new();
        properties.insert("data".to_string(), Value::Object(data_props));
        json!({
            "type": "object",
            "properties": properties,
            "required": ["data"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    async fn execute(&self, input: Value, _context: &ToolContext) -> Result<ToolResult, ToolError> {
        self.execute_inner(input).await
    }
}

impl JsonSchemaTerminator {
    /// Core submission logic, decoupled from [`ToolContext`] so it can be unit
    /// tested without building a full engine context. Validates `input`'s
    /// `data` against the schema, records a conformant submission in the shared
    /// slot, and tags the result with [`TERMINATE_MARKER`].
    pub async fn execute_inner(&self, input: Value) -> Result<ToolResult, ToolError> {
        let data = input
            .get("data")
            .cloned()
            .ok_or_else(|| ToolError::invalid_input("`data` (the result object) is required"))?;

        match self.validate(&data) {
            Ok(()) => {
                // Record the accepted submission so the exec loop can surface it.
                if let Ok(mut guard) = self.submission.lock() {
                    *guard = Some(data.clone());
                }
                Ok(ToolResult {
                    content: serde_json::to_string_pretty(&data)
                        .map_err(|e| ToolError::execution_failed(e.to_string()))?,
                    success: true,
                    metadata: Some(json!({ TERMINATE_MARKER: true })),
                })
            }
            Err(violation) => Err(ToolError::invalid_input(format!(
                "submission does not match --json-schema: {violation}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema() -> Value {
        json!({
            "type": "object",
            "required": ["name", "age"],
            "properties": {
                "name": { "type": "string" },
                "age": { "type": "integer" },
                "role": { "type": "string", "enum": ["admin", "user"] }
            }
        })
    }

    #[test]
    fn accepts_conformant_submission_and_sets_flag() {
        let slot: SubmissionSlot = Arc::new(Mutex::new(None));
        let tool = JsonSchemaTerminator::new(schema(), slot.clone());
        let input = json!({ "data": { "name": "alice", "age": 30, "role": "admin" } });
        let result = block_on(async { tool.execute_inner(input).await });
        assert!(result.is_ok(), "expected ok, got {result:?}");
        let result = result.unwrap();
        assert!(result.success);
        assert_eq!(
            result.metadata.as_ref().and_then(|m| m.get(TERMINATE_MARKER)).as_deref(),
            Some(&json!(true))
        );
        let stored = slot.lock().unwrap();
        assert_eq!(stored.as_ref().unwrap()["name"], "alice");
    }

    #[test]
    fn rejects_non_conformant_submission_with_error() {
        let slot: SubmissionSlot = Arc::new(Mutex::new(None));
        let tool = JsonSchemaTerminator::new(schema(), slot.clone());
        let input = json!({ "data": { "name": "alice", "age": "thirty" } });
        let result = block_on(async { tool.execute_inner(input).await });
        assert!(result.is_err(), "expected rejection, got {result:?}");
        let err = result.unwrap_err().to_string();
        // The validator reports the type mismatch on the offending value; the
        // full path ("age") is surfaced by `jsonschema` as `age` in some
        // builds, but the type error is always present.
        assert!(
            err.contains("integer") || err.contains("age"),
            "error should describe the type violation: {err}"
        );
        // No submission should have been recorded.
        assert!(slot.lock().unwrap().is_none());
    }

    #[test]
    fn rejects_missing_data_field() {
        let slot: SubmissionSlot = Arc::new(Mutex::new(None));
        let tool = JsonSchemaTerminator::new(schema(), slot.clone());
        let input = json!({ "other": 1 });
        let result = block_on(async { tool.execute_inner(input).await });
        assert!(result.is_err());
    }

    #[test]
    fn parse_inline_schema() {
        let v = parse_json_schema_arg(r#"{"type":"object"}"#).unwrap();
        assert_eq!(v["type"], "object");
    }

    #[test]
    fn parse_file_schema() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("schema.json");
        std::fs::write(&path, r#"{"type":"object","required":["x"]}"#).unwrap();
        let v = parse_json_schema_arg(path.to_str().unwrap()).unwrap();
        assert_eq!(v["required"][0], "x");
    }

    #[test]
    fn parse_inline_schema_rejects_garbage() {
        assert!(parse_json_schema_arg("not json and not a path").is_err());
    }

    // Tiny single-threaded block_on so we can drive the async `execute_inner`
    // from a plain `#[test]` without a shared runtime.
    fn block_on<F: std::future::Future>(fut: F) -> F::Output {
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        rt.block_on(fut)
    }
}
