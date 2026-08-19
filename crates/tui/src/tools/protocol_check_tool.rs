//! Protocol state-machine check tool: `protocol_check`.
//!
//! Wires the typestate / protocol FSM engine ([`mimofan_staticanalysis::
//! typestate`]) into the tool surface. Many vulnerabilities are *ordering*
//! bugs: a sensitive operation is only safe after the object has passed
//! through a required setup state (e.g. a deserialization protocol must see
//! `safeMode` engaged before `readObject`). This tool loads the declarative
//! protocol FSMs (rules/protocols/*.yaml) and, given a method-call sequence
//! observed on a tracked object, reports every protocol-violation (a guarded
//! method called in the wrong state).
//!
//! The engine existed with ZERO TUI callers before this tool (plan `plans/12`
//! Phase 3). The call-graph analyzer tracks callee *names* but not the
//! receiver object a call is made on, so receiver-aware sequence extraction is
//! deliberately left to the model / an upstream analyzer: the model supplies
//! the ordered call sequence (method name + line), this tool checks it against
//! every loaded protocol FSM and returns the violations. Read-only; never
//! writes, executes, or networks.

use async_trait::async_trait;
use mimofan_staticanalysis::typestate::{ProtocolFsm, load_protocols_dir};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec,
};

/// Tool name for the model-facing API.
pub const PROTOCOL_CHECK_TOOL_NAME: &str = "protocol_check";

/// On-disk protocols dir, resolved relative to the staticanalysis crate.
const PROTOCOLS_DIR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../staticanalysis/src/rules/protocols"
);

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CallStep {
    method: String,
    line: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProtocolCheckInput {
    /// Ordered sequence of method calls observed on a tracked object, e.g.
    /// `[{ "method": "readObject", "line": 42 }]`. The model or an upstream
    /// analyzer extracts this from source; this tool checks the ordering.
    call_sequence: Vec<CallStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ViolationOut {
    protocol: String,
    object: String,
    method: String,
    line: usize,
    message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProtocolCheckOutput {
    protocols_loaded: usize,
    sequence_len: usize,
    violation_count: usize,
    violations: Vec<ViolationOut>,
}

/// Check an ordered method-call sequence against the protocol state machines.
pub struct ProtocolCheckTool;

impl ProtocolCheckTool {
    /// Core logic shared by `execute` and the unit test.
    fn run_check(input: &ProtocolCheckInput) -> Result<ProtocolCheckOutput, ToolError> {
        let protocols: Vec<ProtocolFsm> = load_protocols_dir(PROTOCOLS_DIR)
            .map_err(|e| ToolError::execution_failed(format!("load protocols: {e}")))?;

        let calls: Vec<(String, usize)> = input
            .call_sequence
            .iter()
            .map(|c| (c.method.clone(), c.line))
            .collect();

        let mut violations: Vec<ViolationOut> = Vec::new();
        for p in &protocols {
            for v in p.check_sequence(&calls) {
                violations.push(ViolationOut {
                    protocol: v.protocol,
                    object: v.object,
                    method: v.method,
                    line: v.at_line,
                    message: v.message,
                });
            }
        }

        Ok(ProtocolCheckOutput {
            protocols_loaded: protocols.len(),
            sequence_len: calls.len(),
            violation_count: violations.len(),
            violations,
        })
    }
}

#[async_trait]
impl ToolSpec for ProtocolCheckTool {
    fn name(&self) -> &'static str {
        PROTOCOL_CHECK_TOOL_NAME
    }

    fn description(&self) -> &'static str {
        "Check an ordered method-call sequence against the declarative protocol \
         state machines (typestate): flags ordering bugs where a guarded method \
         (e.g. readObject) is called before its required state (e.g. safeMode). \
         Provide `call_sequence` as [{method, line}]. Read-only and offline."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "call_sequence": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "method": { "type": "string", "description": "Method name called." },
                            "line": { "type": "integer", "description": "Source line of the call." }
                        },
                        "required": ["method"]
                    },
                    "description": "Ordered method-call sequence observed on a tracked object."
                }
            },
            "required": ["call_sequence"],
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

        if parsed.call_sequence.is_empty() {
            return Err(ToolError::invalid_input(
                "protocol_check requires a non-empty 'call_sequence'",
            ));
        }

        let output = Self::run_check(&parsed)?;
        ToolResult::json(&output).map_err(|e| ToolError::execution_failed(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_readobject_before_safemode() {
        // readObject before enableSafeMode violates the deserialization guard.
        let input = ProtocolCheckInput {
            call_sequence: vec![
                CallStep {
                    method: "readObject".into(),
                    line: 10,
                },
                CallStep {
                    method: "enableSafeMode".into(),
                    line: 12,
                },
            ],
        };
        let out = ProtocolCheckTool::run_check(&input).expect("check should succeed");
        assert!(out.protocols_loaded >= 1, "the deserialization FSM must load");
        assert!(
            out.violation_count >= 1,
            "readObject before safeMode must be flagged"
        );
        assert!(
            out.violations
                .iter()
                .any(|v| v.method == "readObject" && v.line == 10),
            "violation should name readObject at line 10, got: {:?}",
            out.violations
        );
    }

    #[test]
    fn no_violation_when_safemode_first() {
        let input = ProtocolCheckInput {
            call_sequence: vec![
                CallStep {
                    method: "enableSafeMode".into(),
                    line: 5,
                },
                CallStep {
                    method: "readObject".into(),
                    line: 7,
                },
            ],
        };
        let out = ProtocolCheckTool::run_check(&input).expect("check should succeed");
        assert_eq!(
            out.violation_count, 0,
            "enableSafeMode before readObject must be clean"
        );
    }
}
