//! Structured-output / syntheticOutput capability (issue #729).
//!
//! Forces the model to emit JSON that conforms to a caller-supplied
//! JSON-Schema, parses it, validates it, and — on validation failure — feeds
//! the error back into the prompt and retries (up to `max_retries`). This makes
//! model output reliably machine-parseable for eval/verifier/external tooling.
//!
//! The schema validator is a self-contained, dependency-free subset covering
//! `type`, `required`, `properties`, `enum`, `items`, and nesting — enough for
//! the nested / enum / required acceptance criteria without pulling in a full
//! JSON-Schema engine.

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::client::ApiClient;
use crate::config::Config;
use crate::llm_client::LlmClient;
use crate::models::{ContentBlock, Message, MessageRequest, SystemPrompt};
use crate::tools::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec,
};

/// Validate `value` against a (subset) JSON-Schema. Returns the first violation
/// as an error string, or `Ok(())` when conformant.
pub fn validate_against_schema(value: &Value, schema: &Value) -> Result<(), String> {
    validate_node(value, schema, "root")
}

fn validate_node(value: &Value, schema: &Value, path: &str) -> Result<(), String> {
    let Some(schema_type) = schema.get("type").and_then(Value::as_str) else {
        // No explicit type constraint — accept.
        return Ok(());
    };

    match schema_type {
        "object" => {
            let Value::Object(map) = value else {
                return Err(format!("{path}: expected object, got {}", type_name(value)));
            };
            if let Some(required) = schema.get("required").and_then(Value::as_array) {
                for req in required {
                    if let Some(field) = req.as_str() {
                        if !map.contains_key(field) {
                            return Err(format!("{path}: missing required field '{field}'"));
                        }
                    }
                }
            }
            if let Some(props) = schema.get("properties").and_then(Value::as_object) {
                for (key, sub) in props {
                    if let Some(child) = map.get(key) {
                        validate_node(child, sub, &format!("{path}.{key}"))?;
                    }
                }
            }
            Ok(())
        }
        "array" => {
            let Value::Array(items) = value else {
                return Err(format!("{path}: expected array, got {}", type_name(value)));
            };
            if let Some(item_schema) = schema.get("items") {
                for (i, item) in items.iter().enumerate() {
                    validate_node(item, item_schema, &format!("{path}[{i}]"))?;
                }
            }
            Ok(())
        }
        "string" => {
            if !value.is_string() {
                return Err(format!("{path}: expected string, got {}", type_name(value)));
            }
            check_enum(value, schema, path)
        }
        "integer" => {
            if !value.is_u64() && !value.is_i64() {
                return Err(format!(
                    "{path}: expected integer, got {}",
                    type_name(value)
                ));
            }
            check_enum(value, schema, path)
        }
        "number" => {
            if !value.is_number() {
                return Err(format!("{path}: expected number, got {}", type_name(value)));
            }
            check_enum(value, schema, path)
        }
        "boolean" => {
            if !value.is_boolean() {
                return Err(format!(
                    "{path}: expected boolean, got {}",
                    type_name(value)
                ));
            }
            check_enum(value, schema, path)
        }
        other => Err(format!("{path}: unsupported schema type '{other}'")),
    }
}

fn check_enum(value: &Value, schema: &Value, path: &str) -> Result<(), String> {
    if let Some(enum_values) = schema.get("enum").and_then(Value::as_array) {
        if !enum_values.iter().any(|v| v == value) {
            return Err(format!(
                "{path}: value {} not in allowed enum {:?}",
                value, enum_values
            ));
        }
    }
    Ok(())
}

fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Build the user prompt for a given attempt, folding prior validation errors
/// back in so the model can self-correct (issue #729 retry-with-feedback).
pub fn build_attempt_prompt(base_prompt: &str, schema: &Value, prior_errors: &[String]) -> String {
    let schema_text = serde_json::to_string_pretty(schema).unwrap_or_default();
    let mut out = String::new();
    out.push_str(base_prompt);
    out.push_str("\n\nRespond with a single JSON object conforming exactly to this schema:\n");
    out.push_str(&schema_text);
    out.push('\n');
    if !prior_errors.is_empty() {
        out.push_str("\nYour previous attempt failed validation. Fix these errors:\n");
        for err in prior_errors {
            out.push_str(&format!("- {err}\n"));
        }
    }
    out
}

/// Run the structured-output loop. `call_model` is injected so tests can supply
/// canned responses; production callers pass [`call_model_real`].
pub async fn run_structured<F, Fut>(
    base_prompt: &str,
    schema: &Value,
    _model: &str,
    _config: &Config,
    max_retries: usize,
    mut call_model: F,
) -> Result<Value, String>
where
    F: FnMut(String) -> Fut,
    Fut: std::future::Future<Output = Result<String, String>>,
{
    let mut errors: Vec<String> = Vec::new();
    for attempt in 0..=max_retries {
        let prompt = build_attempt_prompt(base_prompt, schema, &errors);
        let raw = call_model(prompt).await.map_err(|e| e)?;
        let parsed: Value = match serde_json::from_str(&raw) {
            Ok(v) => v,
            Err(e) => {
                errors.push(format!("response was not valid JSON: {e}"));
                if attempt == max_retries {
                    return Err(format!(
                        "model output could not be parsed after {max_retries} retries: {e}"
                    ));
                }
                continue;
            }
        };
        match validate_against_schema(&parsed, schema) {
            Ok(()) => return Ok(parsed),
            Err(violation) => {
                errors.push(violation.clone());
                if attempt == max_retries {
                    return Err(format!(
                        "schema validation failed after {max_retries} retries: {violation}"
                    ));
                }
            }
        }
    }
    Err("exhausted retries without valid output".to_string())
}

/// Production model caller: routes through the same auto-route machinery as the
/// CLI and one-shot paths, then extracts the text reply.
pub async fn call_model_real(
    prompt: String,
    model: &str,
    config: &Config,
) -> Result<String, String> {
    let route = crate::resolve_cli_auto_route(config, model, &prompt)
        .await
        .map_err(|e| e.to_string())?;
    let execution_config = crate::config_for_cli_route(config, &route);
    let client = ApiClient::new(&execution_config).map_err(|e| e.to_string())?;
    let request = MessageRequest {
        model: route.model,
        messages: vec![Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: prompt,
                cache_control: None,
            }],
        }],
        max_tokens: 4096,
        system: Some(SystemPrompt::Text(
            "You emit strictly valid JSON matching the requested schema. No prose, no markdown fences."
                .to_string(),
        )),
        tools: None,
        tool_choice: None,
        metadata: None,
        thinking: None,
        reasoning_effort: None,
        stream: Some(false),
        temperature: Some(0.2),
        top_p: Some(0.9),
        response_format: Some(json!({
            "type": "json_schema",
            "json_schema": { "name": "synthetic_output", "strict": true }
        })),
    };
    let response = client
        .create_message(request)
        .await
        .map_err(|e| e.to_string())?;
    let mut out = String::new();
    for block in response.content {
        if let ContentBlock::Text { text, .. } = block {
            out.push_str(&text);
        }
    }
    Ok(out)
}

pub struct SyntheticOutputTool;

#[async_trait]
impl ToolSpec for SyntheticOutputTool {
    fn name(&self) -> &'static str {
        "synthetic_output"
    }

    fn description(&self) -> &'static str {
        "Force the model to emit JSON conforming to a JSON-Schema, validate it, and retry with error feedback on failure. Returns the parsed, schema-valid object. For eval/verifier/external tooling that needs typed model output."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "Instruction for the model."
                },
                "schema": {
                    "type": "object",
                    "description": "JSON-Schema the output must conform to (supports type/required/properties/enum/items, nested)."
                },
                "model": {
                    "type": "string",
                    "description": "Optional model override; defaults to the resolved auto model."
                },
                "max_retries": {
                    "type": "integer",
                    "description": "Validation retry count on failure (default 3, max 10)."
                }
            },
            "required": ["prompt", "schema"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly, ToolCapability::Network]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    async fn execute(&self, input: Value, _context: &ToolContext) -> Result<ToolResult, ToolError> {
        let prompt = input
            .get("prompt")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::invalid_input("`prompt` is required"))?
            .to_string();
        let schema = input
            .get("schema")
            .cloned()
            .ok_or_else(|| ToolError::invalid_input("`schema` is required"))?;

        let max_retries = input
            .get("max_retries")
            .and_then(Value::as_u64)
            .unwrap_or(3)
            .min(10) as usize;

        // Validate the schema itself up-front so we fail fast on a bad contract.
        if schema.get("type").is_none() {
            return Err(ToolError::invalid_input(
                "`schema` must declare at least a top-level `type`",
            ));
        }

        let config = Config::load(None, None)
            .map_err(|e| ToolError::execution_failed(format!("failed to load config: {e}")))?;
        let model = input
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or("deepseek")
            .to_string();

        match run_structured(&prompt, &schema, &model, &config, max_retries, |p| {
            call_model_real(p, &model, &config)
        })
        .await
        {
            Ok(value) => Ok(ToolResult {
                content: serde_json::to_string_pretty(&value)
                    .map_err(|e| ToolError::execution_failed(e.to_string()))?,
                success: true,
                metadata: None,
            }),
            Err(e) => Err(ToolError::execution_failed(format!(
                "synthetic_output failed: {e}"
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
    fn validates_conformant_object() {
        let v = json!({ "name": "alice", "age": 30, "role": "admin" });
        assert!(validate_against_schema(&v, &schema()).is_ok());
    }

    #[test]
    fn rejects_missing_required() {
        let v = json!({ "name": "alice" });
        let err = validate_against_schema(&v, &schema()).unwrap_err();
        assert!(err.contains("age"), "got: {err}");
    }

    #[test]
    fn rejects_wrong_type() {
        let v = json!({ "name": "alice", "age": "thirty" });
        let err = validate_against_schema(&v, &schema()).unwrap_err();
        assert!(err.contains("age"), "got: {err}");
    }

    #[test]
    fn rejects_enum_violation() {
        let v = json!({ "name": "alice", "age": 1, "role": "superuser" });
        let err = validate_against_schema(&v, &schema()).unwrap_err();
        assert!(err.contains("role"), "got: {err}");
    }

    #[test]
    fn validates_nested_array() {
        let s = json!({
            "type": "object",
            "required": ["items"],
            "properties": {
                "items": {
                    "type": "array",
                    "items": { "type": "integer" }
                }
            }
        });
        assert!(validate_against_schema(&json!({ "items": [1, 2, 3] }), &s).is_ok());
        assert!(validate_against_schema(&json!({ "items": [1, "x"] }), &s).is_err());
    }

    #[tokio::test]
    async fn retries_with_error_feedback_then_succeeds() {
        // First response is invalid (missing required `age`), second is valid.
        let responses = vec![
            Ok(r#"{"name":"bob"}"#.to_string()),
            Ok(r#"{"name":"bob","age":42}"#.to_string()),
        ];
        let mut it = responses.into_iter();
        let result = run_structured(
            "make a person",
            &schema(),
            "deepseek",
            &Config::load(None, None).unwrap(),
            3,
            |_p| {
                let next = it.next().unwrap();
                async move { next }
            },
        )
        .await;
        assert!(
            result.is_ok(),
            "expected success after retry, got {result:?}"
        );
        assert_eq!(result.unwrap()["age"], 42);
    }

    #[tokio::test]
    async fn exhausts_retries_on_persistent_failure() {
        let responses = vec![
            Ok(r#"{"name":"x"}"#.to_string()),
            Ok(r#"{"name":"x"}"#.to_string()),
            Ok(r#"{"name":"x"}"#.to_string()),
            Ok(r#"{"name":"x"}"#.to_string()),
        ];
        let mut it = responses.into_iter();
        let result = run_structured(
            "make a person",
            &schema(),
            "deepseek",
            &Config::load(None, None).unwrap(),
            3,
            |_p| {
                let next = it.next().unwrap();
                async move { next }
            },
        )
        .await;
        assert!(result.is_err(), "should fail after exhausting retries");
    }

    #[test]
    fn build_attempt_prompt_includes_errors_after_first() {
        let p0 = build_attempt_prompt("do it", &schema(), &[]);
        assert!(!p0.contains("previous attempt failed"));
        let p1 = build_attempt_prompt(
            "do it",
            &schema(),
            &["root: missing required field 'age'".to_string()],
        );
        assert!(p1.contains("missing required field 'age'"));
    }
}
