//! Protocol / typestate FSM check tool: `protocol_check`.
//!
//! Wraps the `staticanalysis::typestate` engine (protocol state machines loaded
//! from `src/rules/protocols/*.yaml`) so the model can detect *ordering* bugs:
//! a guarded method (e.g. `readObject`) called before its required setup state
//! (`safe_mode`) is reached. The tool loads the shipped protocol FSMs, scans the
//! target's source files for method calls on each protocol's tracked object, and
//! reports any [`ProtocolViolation`] the FSM forbids.
//!
//! The call-sequence extraction is a **naive, best-effort** source scan (regex
//! over `.rs`/`.java`/`.ts`/`.js` lines matching `object.method(...)`); it is
//! not a full call-graph, so cross-procedure ordering is out of scope. The
//! engine itself (`typestate.rs`) is reused unchanged.
use std::collections::HashSet;
use std::path::Path;

use async_trait::async_trait;
use mimofan_staticanalysis::typestate::{ProtocolFsm, ProtocolViolation, load_protocols_dir};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec,
};

/// Tool name for the model-facing API.
pub const PROTOCOL_CHECK_TOOL_NAME: &str = "protocol_check";

/// On-disk protocol FSM directory, resolved relative to the staticanalysis
/// crate. The tui crate sits at `crates/tui`, so `../staticanalysis` reaches the
/// sibling crate whose `src/rules/protocols` holds the YAML state machines.
const PROTOCOLS_DIR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../staticanalysis/src/rules/protocols"
);

/// Check protocol state-machine ordering in a target directory.
pub struct ProtocolCheckTool;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProtocolCheckInput {
    /// Root directory to scan. Required.
    #[serde(default)]
    target_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProtocolView {
    name: String,
    object: String,
    initial: String,
    accepting: Vec<String>,
    states: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ViolationView {
    protocol: String,
    object: String,
    method: String,
    line: usize,
    message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProtocolCheckOutput {
    protocols: Vec<ProtocolView>,
    violations: Vec<ViolationView>,
}

/// The source-file extensions we scan for method-call sequences.
const SOURCE_EXTENSIONS: &[&str] = &["rs", "java", "ts", "js"];

/// All method names a protocol's FSM knows about (transitions `.on` + guards
/// `.on`). These are the only methods we match when building call sequences.
fn known_methods(fsm: &ProtocolFsm) -> HashSet<String> {
    let mut m = HashSet::new();
    for t in &fsm.transitions {
        m.insert(t.on.clone());
    }
    for g in &fsm.guards {
        m.insert(g.on.clone());
    }
    m
}

/// Recursively collect call sequences `(method, line)` for a protocol's tracked
/// object from every source file under `root`. Naive line scan:
/// `object.method(` or `object .method(`.
fn collect_calls(root: &Path, object: &str, methods: &HashSet<String>) -> Vec<(String, usize)> {
    let mut calls = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let path = e.path();
            if path.is_dir() {
                stack.push(path);
            } else if path
                .extension()
                .and_then(|x| x.to_str())
                .map(|ext| SOURCE_EXTENSIONS.contains(&ext))
                .unwrap_or(false)
            {
                collect_from_file(&path, object, methods, &mut calls);
            }
        }
    }
    calls
}

fn collect_from_file(
    path: &Path,
    object: &str,
    methods: &HashSet<String>,
    out: &mut Vec<(String, usize)>,
) {
    let Ok(content) = std::fs::read_to_string(path) else {
        return;
    };
    for (idx, line) in content.lines().enumerate() {
        let lineno = idx + 1;
        // Naive in-scope heuristic: only consider lines that mention the
        // tracked object name (a variable of that type is in scope here). Then
        // scan left-to-right for known-method calls `.method(`, preserving
        // source order.
        if !line.contains(object) {
            continue;
        }
        let bytes = line.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'.' {
                // Extract the token after the dot up to `(` or a non-ident char.
                let mut j = i + 1;
                while j < bytes.len()
                    && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_')
                {
                    j += 1;
                }
                let name = &line[i + 1..j];
                if j < bytes.len() && bytes[j] == b'(' && methods.contains(name) {
                    out.push((name.to_string(), lineno));
                }
                i = j;
            } else {
                i += 1;
            }
        }
    }
}

fn to_protocol_view(fsm: &ProtocolFsm) -> ProtocolView {
    ProtocolView {
        name: fsm.protocol.clone(),
        object: fsm.object.clone(),
        initial: fsm.initial.clone(),
        accepting: fsm.accepting.clone(),
        states: fsm.states.clone(),
    }
}

fn to_violation_view(v: &ProtocolViolation) -> ViolationView {
    ViolationView {
        protocol: v.protocol.clone(),
        object: v.object.clone(),
        method: v.method.clone(),
        line: v.at_line,
        message: v.message.clone(),
    }
}

/// Load the protocol FSMs and check each against the extracted call sequences.
/// Pure function so it can run inside `spawn_blocking`.
fn run_protocol_check(target_dir: &str) -> Result<ProtocolCheckOutput, anyhow::Error> {
    let fsms = load_protocols_dir(PROTOCOLS_DIR)?;
    let root = Path::new(target_dir);
    let mut protocols = Vec::new();
    let mut violations = Vec::new();
    for fsm in &fsms {
        protocols.push(to_protocol_view(fsm));
        if fsm.object.is_empty() {
            continue;
        }
        let methods = known_methods(fsm);
        let calls = collect_calls(root, &fsm.object, &methods);
        for v in fsm.check_sequence(&calls) {
            violations.push(to_violation_view(&v));
        }
    }
    Ok(ProtocolCheckOutput {
        protocols,
        violations,
    })
}

#[async_trait]
impl ToolSpec for ProtocolCheckTool {
    fn name(&self) -> &'static str {
        PROTOCOL_CHECK_TOOL_NAME
    }

    fn description(&self) -> &'static str {
        "Check protocol state-machine (typestate FSM) ordering violations in a target \
         directory. Loads the shipped protocol FSMs (e.g. deserialization guard) and scans \
         source files for method calls on each protocol's tracked object, reporting calls \
         made before the required setup state was reached. Read-only: no code runs, no files \
         are written, no network is used."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "target_dir": {
                    "type": "string",
                    "description": "Root directory to scan. Required."
                }
            },
            "required": ["target_dir"],
            "additionalProperties": false
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    fn supports_parallel(&self) -> bool {
        true
    }

    async fn execute(&self, input: Value, _context: &ToolContext) -> Result<ToolResult, ToolError> {
        let parsed: ProtocolCheckInput = serde_json::from_value(input.clone()).map_err(|e| {
            ToolError::invalid_input(format!("invalid protocol_check input: {e}"))
        })?;

        if parsed.target_dir.trim().is_empty() {
            return Err(ToolError::invalid_input(
                "protocol_check requires a non-empty 'target_dir'",
            ));
        }

        let target_dir = parsed.target_dir.clone();
        let output = tokio::task::spawn_blocking(move || run_protocol_check(&target_dir))
            .await
            .map_err(|e| ToolError::execution_failed(format!("task join: {e}")))?
            .map_err(|e| {
                ToolError::execution_failed(format!("protocol_check failed: {e}"))
            })?;

        ToolResult::json(&output).map_err(|e| ToolError::execution_failed(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_is_protocol_check() {
        let tool = ProtocolCheckTool;
        assert_eq!(tool.name(), "protocol_check");
    }

    #[test]
    fn input_schema_requires_target_dir() {
        let tool = ProtocolCheckTool;
        let schema = tool.input_schema();
        let required = schema["required"].as_array().expect("required array");
        assert!(
            required.iter().any(|v| v.as_str() == Some("target_dir")),
            "input schema must require 'target_dir'"
        );
        assert!(
            schema["properties"]["target_dir"]["type"].as_str() == Some("string"),
            "target_dir must be a string property"
        );
    }

    #[test]
    fn loads_shipped_protocols() {
        // The on-disk protocol FSMs must be real, not a stub.
        let fsms = load_protocols_dir(PROTOCOLS_DIR).expect("load protocols dir");
        assert!(
            !fsms.is_empty(),
            "expected at least one protocol FSM on disk"
        );
        assert!(
            fsms.iter().any(|f| f.protocol == "deserialization"),
            "deserialization protocol must be present"
        );
    }

    #[test]
    fn naive_scan_finds_deserialization_violation() {
        // A temp source file calling readObject before enableSafeMode. The
        // object name and both calls sit on one line so the naive in-scope
        // heuristic matches them.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("app.rs"),
            "let s = SafeObjectInputStream::new(); s.readObject(data); s.enableSafeMode();\n",
        )
        .unwrap();

        let fsm = ProtocolFsm::from_yaml(
            "deser.yaml",
            "protocol: deserialization\n\
             object: SafeObjectInputStream\n\
             initial: created\n\
             accepting: [ready]\n\
             states: [created, safe_mode, ready, poisoned]\n\
             transitions:\n\
             \x20 - from: created\n\
             \x20   on: enableSafeMode\n\
             \x20   to: safe_mode\n\
             \x20 - from: safe_mode\n\
             \x20   on: readObject\n\
             \x20   to: ready\n\
             guards:\n\
             \x20 - on: readObject\n\
             \x20   require_state: safe_mode\n",
        )
        .unwrap();

        let methods = known_methods(&fsm);
        let calls = collect_calls(dir.path(), &fsm.object, &methods);
        let violations = fsm.check_sequence(&calls);
        assert!(
            !violations.is_empty(),
            "readObject before safe mode must be a violation, got: {calls:?}"
        );
        assert!(violations[0].method == "readObject");
    }
}
