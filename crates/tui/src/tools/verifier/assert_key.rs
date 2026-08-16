//! Tool-level `assert_key` runtime verifier (#852).
//!
//! A tool can declare, via [`AssertKey`], the set of keys its JSON output
//! **MUST** contain (`required`) and the keys it must **NOT** contain
//! (`forbid`). [`verify_output`] checks a tool's [`serde_json::Value`] output
//! against that spec at runtime, returning an [`AssertError`] that lists
//! exactly which required keys were missing and which forbidden keys were
//! present.
//!
//! Tools opt into runtime validation by implementing [`ToolVerifier`]; a
//! typical tool keeps an `AssertKey` field and calls
//! `self.assert.verify(&output)` at the end of `execute` before returning.
//! This module is pure (no IO, no LLM) and fully unit-testable.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

/// Declares the structural contract for a tool's output.
///
/// `required` keys must all be present in the output object (any non-null,
/// defined value satisfies presence — including `false`, `0`, and empty
/// strings/arrays). `forbid` keys must be absent. Dot-path keys
/// (`"a.b.c"`) descend into nested objects; a missing intermediate object
/// counts as "not present" (so a required nested key is reported missing, and
/// a forbidden nested key is considered absent/satisfied).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssertKey {
    /// Keys that MUST be present in the tool output.
    #[serde(default)]
    pub required: Vec<String>,
    /// Keys that MUST NOT be present in the tool output.
    #[serde(default)]
    pub forbid: Vec<String>,
}

impl AssertKey {
    /// Create an empty contract (accepts any output).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a contract requiring the given keys.
    #[must_use]
    pub fn require(required: Vec<String>) -> Self {
        Self {
            required,
            forbid: Vec::new(),
        }
    }

    /// Create a contract forbidding the given keys.
    #[must_use]
    pub fn forbid(keys: Vec<String>) -> Self {
        Self {
            required: Vec::new(),
            forbid: keys,
        }
    }

    /// Add required keys, returning `self` for chaining.
    #[must_use]
    pub fn with_required(mut self, required: Vec<String>) -> Self {
        self.required.extend(required);
        self
    }

    /// Add forbidden keys, returning `self` for chaining.
    #[must_use]
    pub fn with_forbidden(mut self, forbid: Vec<String>) -> Self {
        self.forbid.extend(forbid);
        self
    }

    /// Whether this contract is empty (accepts any output).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.required.is_empty() && self.forbid.is_empty()
    }
}

/// Error returned when a tool output violates its [`AssertKey`] contract.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AssertError {
    /// The output was not a JSON object, so key-based checks are impossible.
    #[error("assert_key: expected a JSON object output, got {kind}")]
    NotAnObject { kind: &'static str },

    /// One or more required keys were missing and/or forbidden keys present.
    #[error(
        "assert_key failed: missing required keys {missing:?}, forbidden keys present {present:?}"
    )]
    Violated {
        missing: Vec<String>,
        present: Vec<String>,
    },
}

/// Resolve a (possibly dot-path) key against a JSON value, descending into
/// nested objects. Returns `Some(value)` when present, `None` when the key or
/// any intermediate object is missing.
fn resolve<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    let mut current = value;
    for segment in key.split('.') {
        match current {
            Value::Object(map) => {
                current = map.get(segment)?;
            }
            _ => return None,
        }
    }
    Some(current)
}

/// Whether a JSON value counts as "present" for the purposes of a required
/// key. We treat only `undefined`/`null` as absent; `false`, `0`, `""`, `[]`,
/// and `{}` are all considered present (a defined value satisfies presence).
fn is_present(value: &Value) -> bool {
    !value.is_null()
}

/// Validate `output` against the [`AssertKey`] contract.
///
/// # Errors
/// Returns [`AssertError::NotAnObject`] when `output` is not an object, and
/// [`AssertError::Violated`] listing every missing required key and every
/// present forbidden key otherwise. When both lists are empty the output
/// satisfies the contract and `Ok(())` is returned.
pub fn verify_output(assert: &AssertKey, output: &Value) -> Result<(), AssertError> {
    let Value::Object(_) = output else {
        let kind = match output {
            Value::Array(_) => "array",
            Value::String(_) => "string",
            Value::Number(_) => "number",
            Value::Bool(_) => "bool",
            Value::Null => "null",
            _ => "unknown",
        };
        return Err(AssertError::NotAnObject { kind });
    };

    if assert.is_empty() {
        return Ok(());
    }

    let mut missing = Vec::new();
    for key in &assert.required {
        match resolve(output, key) {
            Some(value) if is_present(value) => {}
            _ => missing.push(key.clone()),
        }
    }

    let mut present = Vec::new();
    for key in &assert.forbid {
        if let Some(value) = resolve(output, key) {
            if is_present(value) {
                present.push(key.clone());
            }
        }
    }

    if missing.is_empty() && present.is_empty() {
        Ok(())
    } else {
        Err(AssertError::Violated { missing, present })
    }
}

/// A tool that can validate its own output at runtime against an
/// [`AssertKey`] contract.
///
/// Tools opt in by implementing this trait; the default [`ToolVerifier::verify`]
/// simply delegates to [`verify_output`] with the tool's declared
/// [`ToolVerifier::assert_key`]. Implementors override [`ToolVerifier::verify`]
/// only when they need custom logic beyond the key contract.
pub trait ToolVerifier {
    /// The structural contract this tool's output must satisfy.
    fn assert_key(&self) -> AssertKey;

    /// Validate `output` against this tool's contract.
    ///
    /// The default implementation delegates to [`verify_output`]. Tools with
    /// no contract (empty [`AssertKey`]) accept any object output.
    fn verify(&self, output: &Value) -> Result<(), AssertError> {
        verify_output(&self.assert_key(), output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj() -> Value {
        serde_json::json!({
            "status": "ok",
            "result": { "id": 42 },
            "items": [],
            "found": false
        })
    }

    #[test]
    fn happy_path_all_required_present_no_forbidden() {
        let assert = AssertKey::require(vec!["status".to_string(), "result".to_string()])
            .with_forbidden(vec!["error".to_string()]);
        let result = verify_output(&assert, &obj());
        assert!(result.is_ok(), "valid output must pass: {result:?}");
    }

    #[test]
    fn missing_required_key_reported() {
        let assert = AssertKey::require(vec!["status".to_string(), "missing".to_string()]);
        let err = verify_output(&assert, &obj()).expect_err("missing key must fail");
        match err {
            AssertError::Violated { missing, .. } => {
                assert_eq!(missing, vec!["missing".to_string()]);
            }
            other => panic!("expected Violated, got {other:?}"),
        }
    }

    #[test]
    fn forbidden_key_present_reported() {
        let assert = AssertKey::forbid(vec!["status".to_string()]);
        let err = verify_output(&assert, &obj()).expect_err("forbidden key must fail");
        match err {
            AssertError::Violated { present, .. } => {
                assert_eq!(present, vec!["status".to_string()]);
            }
            other => panic!("expected Violated, got {other:?}"),
        }
    }

    #[test]
    fn both_missing_and_forbidden_reported() {
        let assert =
            AssertKey::require(vec!["nope".to_string()]).with_forbidden(vec!["status".to_string()]);
        let err = verify_output(&assert, &obj()).expect_err("both violations must fail");
        match err {
            AssertError::Violated { missing, present } => {
                assert_eq!(missing, vec!["nope".to_string()]);
                assert_eq!(present, vec!["status".to_string()]);
            }
            other => panic!("expected Violated, got {other:?}"),
        }
    }

    #[test]
    fn false_value_counts_as_present_for_required() {
        // `found: false` is a defined value, so a required `found` key is met.
        let assert = AssertKey::require(vec!["found".to_string()]);
        assert!(verify_output(&assert, &obj()).is_ok());
    }

    #[test]
    fn null_value_counts_as_missing_for_required() {
        let output = serde_json::json!({ "x": null });
        let assert = AssertKey::require(vec!["x".to_string()]);
        assert!(verify_output(&assert, &output).is_err());
    }

    #[test]
    fn empty_contract_accepts_any_object() {
        let assert = AssertKey::new();
        assert!(verify_output(&assert, &obj()).is_ok());
        assert!(verify_output(&assert, &serde_json::json!({})).is_ok());
    }

    #[test]
    fn not_an_object_is_rejected() {
        let assert = AssertKey::require(vec!["status".to_string()]);
        for bad in [
            Value::String("hi".into()),
            Value::Array(vec![]),
            Value::Number(1.into()),
            Value::Bool(true),
            Value::Null,
        ] {
            let err = verify_output(&assert, &bad).expect_err("non-object must fail");
            assert!(matches!(err, AssertError::NotAnObject { .. }));
        }
    }

    #[test]
    fn dot_path_required_nested_key() {
        let assert = AssertKey::require(vec!["result.id".to_string()]);
        assert!(verify_output(&assert, &obj()).is_ok());
        let assert_missing = AssertKey::require(vec!["result.absent".to_string()]);
        assert!(verify_output(&assert_missing, &obj()).is_err());
    }

    #[test]
    fn dot_path_forbidden_nested_key() {
        let assert = AssertKey::forbid(vec!["result.id".to_string()]);
        assert!(verify_output(&assert, &obj()).is_err());
        let assert_ok = AssertKey::forbid(vec!["result.absent".to_string()]);
        assert!(verify_output(&assert_ok, &obj()).is_ok());
    }

    // A minimal tool implementing ToolVerifier to exercise the trait path.
    struct StubVerifier {
        assert: AssertKey,
    }

    impl ToolVerifier for StubVerifier {
        fn assert_key(&self) -> AssertKey {
            self.assert.clone()
        }
    }

    #[test]
    fn tool_verifier_trait_delegates() {
        let tool = StubVerifier {
            assert: AssertKey::require(vec!["status".to_string()]),
        };
        assert!(tool.verify(&obj()).is_ok());
        let bad = serde_json::json!({ "other": 1 });
        assert!(tool.verify(&bad).is_err());
    }
}
