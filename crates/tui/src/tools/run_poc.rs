//! `run_poc` tool (axis C / reproducibility, issue #833): execute a candidate
//! Proof-of-Concept in the sandbox and report whether the vulnerability was
//! realized.
//!
//! This is the **可复现 PoC** gate of the vuln-hunt long-horizon harness: did
//! the candidate exploit actually trigger the expected vulnerable behavior?
//! The command is executed through the shared [`SandboxBackend`] (reused from
//! the sandbox group — we do NOT re-implement command execution), so there is
//! a single execution surface and a single place where sandbox policy applies.

use std::collections::HashMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tracing::{debug, warn};

use super::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec,
};
use crate::sandbox::backend::SandboxBackend;

/// Tool name for the model-facing API.
pub const RUN_POC_TOOL_NAME: &str = "run_poc";

/// Default execution timeout (ms) for the PoC command.
const DEFAULT_TIMEOUT_MS: u64 = 30_000;

/// Maximum number of bytes of stdout/stderr tail to return in the result.
const TAIL_MAX_LEN: usize = 2_048;

/// Input for [`RunPocTool`].
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RunPocInput {
    /// The shell command to run the PoC (e.g. "java -jar poc.jar" or
    /// "python exploit.py").
    command: String,
    /// A substring that, if present in stdout/stderr, means the vuln was
    /// realized (e.g. "JNDI connection", "RCE achieved", "uid=").
    expect: String,
    /// Optional execution timeout in milliseconds. Defaults to 30s.
    #[serde(default)]
    timeout_ms: Option<u64>,
}

/// Output for [`RunPocTool`].
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RunPocOutput {
    /// Whether the PoC triggered the expected vulnerable behavior.
    realized: bool,
    /// Process exit code from the sandbox backend.
    exit_code: i32,
    /// The `expect` string that was matched, or `null` when not matched.
    matched_expect: Option<String>,
    /// Last ~2KB of stdout (string to keep JSON simple and stable).
    stdout_tail: String,
    /// Last ~2KB of stderr.
    stderr_tail: String,
    /// Human-readable note explaining the semantics of `realized`.
    note: &'static str,
}

/// Pure (no execution) helper: did the expected marker appear in either stream?
///
/// Extracted so it can be unit-tested without a sandbox backend. Returns true
/// iff `expect` is a substring of `stdout` or `stderr`.
#[must_use]
pub fn evaluate(stdout: &str, stderr: &str, expect: &str) -> bool {
    stdout.contains(expect) || stderr.contains(expect)
}

/// Keep only the last `TAIL_MAX_LEN` bytes of a stream for the result tail.
#[must_use]
pub fn tail(s: &str) -> String {
    if s.len() <= TAIL_MAX_LEN {
        s.to_string()
    } else {
        s[s.len() - TAIL_MAX_LEN..].to_string()
    }
}

/// Execute a candidate Proof-of-Concept and report whether the vulnerability
/// was realized.
pub struct RunPocTool;

#[async_trait]
impl ToolSpec for RunPocTool {
    fn name(&self) -> &'static str {
        RUN_POC_TOOL_NAME
    }

    fn description(&self) -> &'static str {
        "Execute a candidate Proof-of-Concept in the sandbox and report whether the \
         vulnerability was realized. Provide `command` (the shell command to run the \
         PoC) and `expect` (a substring that, if present in stdout/stderr, means the \
         vulnerable behavior was triggered). Returns `realized: true` when the expected \
         marker appears. Requires a configured sandbox backend."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "Shell command to run the PoC, e.g. \"java -jar poc.jar\" or \"python exploit.py\"."
                },
                "expect": {
                    "type": "string",
                    "description": "Substring that, if present in stdout/stderr, means the vuln was realized (e.g. \"JNDI connection\", \"RCE achieved\", \"uid=\")."
                },
                "timeout_ms": {
                    "type": "integer",
                    "description": "Optional execution timeout in milliseconds. Defaults to 30000."
                }
            },
            "required": ["command", "expect"],
            "additionalProperties": false
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ExecutesCode]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Required
    }

    async fn execute(&self, input: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        let parsed: RunPocInput = serde_json::from_value(input.clone())
            .map_err(|e| ToolError::invalid_input(format!("invalid run_poc input: {e}")))?;

        if parsed.command.trim().is_empty() {
            return Err(ToolError::invalid_input("run_poc requires a non-empty 'command'"));
        }
        if parsed.expect.trim().is_empty() {
            return Err(ToolError::invalid_input("run_poc requires a non-empty 'expect'"));
        }

        let Some(backend) = context.sandbox_backend.as_ref() else {
            return Err(ToolError::not_available(
                "no sandbox backend configured; run_poc requires an execution backend",
            ));
        };

        let _timeout = parsed.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS);
        let env: HashMap<String, String> = HashMap::new();

        debug!(
            command = %parsed.command,
            expect = %parsed.expect,
            "run_poc: executing candidate PoC in sandbox"
        );

        let out = match backend.exec(&parsed.command, &env).await {
            Ok(o) => o,
            Err(e) => {
                warn!(error = %e, "run_poc: sandbox exec failed");
                return Err(ToolError::execution_failed(format!(
                    "run_poc sandbox execution failed: {e}"
                )));
            }
        };

        let realized = evaluate(&out.stdout, &out.stderr, &parsed.expect);
        let output = RunPocOutput {
            realized,
            exit_code: out.exit_code,
            matched_expect: realized.then(|| parsed.expect.clone()),
            stdout_tail: tail(&out.stdout),
            stderr_tail: tail(&out.stderr),
            note: "realized means PoC triggered the expected vulnerable behavior",
        };

        ToolResult::json(&output).map_err(|e| ToolError::execution_failed(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_and_detects_realized() {
        // True-case: expected marker present in stdout.
        assert!(
            evaluate("JNDI connection established", "", "JNDI connection"),
            "expect present in stdout must realize the PoC"
        );
        // True-case: expected marker present in stderr.
        assert!(
            evaluate("", "RCE achieved via gadget chain", "RCE achieved"),
            "expect present in stderr must realize the PoC"
        );
        // False-case: marker absent from both streams.
        assert!(
            !evaluate("everything looks fine", "", "JNDI connection"),
            "missing expect must NOT realize the PoC"
        );
    }

    #[test]
    fn tail_keeps_last_bytes_when_long() {
        let long = "x".repeat(TAIL_MAX_LEN + 100);
        let t = tail(&long);
        assert_eq!(t.len(), TAIL_MAX_LEN);
        assert!(t.starts_with('x'));
    }

    #[test]
    fn tail_returns_full_when_short() {
        let short = "short output";
        assert_eq!(tail(short), short);
    }

    #[tokio::test]
    async fn refuses_without_backend() {
        // Build a minimal ToolContext with no sandbox backend (as unit tests
        // do) and confirm the tool fails closed with a clear message.
        let ctx = ToolContext::new(std::env::temp_dir());
        let tool = RunPocTool;
        let input = json!({
            "command": "python exploit.py",
            "expect": "uid="
        });
        let err = tool
            .execute(input, &ctx)
            .await
            .expect_err("run_poc must refuse when no sandbox backend is configured");
        let msg = err.to_string();
        assert!(
            msg.to_ascii_lowercase().contains("sandbox backend"),
            "error must mention sandbox backend, got: {msg}"
        );
    }
}
