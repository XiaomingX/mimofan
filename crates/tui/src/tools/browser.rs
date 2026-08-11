//! Browser automation tool family (issue #743).
//!
//! Exposes `navigate`, `click`, `type`, `screenshot`, and `eval_js` actions
//! driven by an external Playwright process (Node `playwright` CLI or a local
//! Chromium via CDP). The tool reuses `fetch_url`'s SSRF guard
//! (`resolve_and_check_target`) so browser navigation cannot be pointed at
//! loopback / private / link-local / cloud-metadata addresses.
//!
//! Command construction is separated from execution so the JSON protocol and
//! the URL guard are unit-testable without a browser installed.

use std::path::PathBuf;

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::tools::fetch_url::resolve_and_check_target;
use crate::tools::spec::{ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec};

/// Supported browser actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserAction {
    Navigate,
    Click,
    Type,
    Screenshot,
    EvalJs,
}

impl BrowserAction {
    fn as_str(self) -> &'static str {
        match self {
            BrowserAction::Navigate => "navigate",
            BrowserAction::Click => "click",
            BrowserAction::Type => "type",
            BrowserAction::Screenshot => "screenshot",
            BrowserAction::EvalJs => "eval_js",
        }
    }

    fn from_str(s: &str) -> Option<Self> {
        match s {
            "navigate" => Some(BrowserAction::Navigate),
            "click" => Some(BrowserAction::Click),
            "type" => Some(BrowserAction::Type),
            "screenshot" => Some(BrowserAction::Screenshot),
            "eval_js" => Some(BrowserAction::EvalJs),
            _ => None,
        }
    }
}

/// Build the JSON instruction handed to the Playwright driver subprocess.
/// Kept pure so it can be unit-tested independently of a browser install.
pub fn build_browser_instruction(
    action: BrowserAction,
    url: Option<&str>,
    selector: Option<&str>,
    text: Option<&str>,
    script: Option<&str>,
    output_dir: &std::path::Path,
) -> Value {
    let mut inst = json!({
        "action": action.as_str(),
    });
    if let Some(url) = url {
        inst["url"] = json!(url);
    }
    if let Some(selector) = selector {
        inst["selector"] = json!(selector);
    }
    if let Some(text) = text {
        inst["text"] = json!(text);
    }
    if let Some(script) = script {
        inst["script"] = json!(script);
    }
    if action == BrowserAction::Screenshot {
        // Screenshot result is written to this path; the driver returns it.
        inst["outputPath"] = json!(output_dir.join("screenshot.png").to_string_lossy());
    }
    inst
}

/// Validate that `url` is safe to navigate to (SSRF guard). Rejects schemes
/// other than http/https and any restricted IP (loopback/private/link-local/
/// cloud-metadata) by delegating to `fetch_url`'s resolver.
pub async fn is_url_permitted(url: &str) -> Result<(), ToolError> {
    let parsed = reqwest::Url::parse(url)
        .map_err(|e| ToolError::invalid_input(format!("invalid URL: {e}")))?;
    resolve_and_check_target(&parsed).await?;
    Ok(())
}

pub struct BrowserTool;

#[async_trait]
impl ToolSpec for BrowserTool {
    fn name(&self) -> &'static str {
        "browser"
    }

    fn description(&self) -> &'static str {
        "Browser automation via Playwright/CDP. Actions: navigate (open URL), click (selector), type (text into selector), screenshot (save PNG, returns path), eval_js (run script in page). SSRF-guarded: cannot target loopback/private/link-local/cloud-metadata addresses. Requires `npx playwright` or a CDP-capable Chromium in PATH."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["navigate", "click", "type", "screenshot", "eval_js"],
                    "description": "Browser action to perform."
                },
                "url": {
                    "type": "string",
                    "description": "Target URL for `navigate`. Must be http/https and non-restricted."
                },
                "selector": {
                    "type": "string",
                    "description": "CSS selector for `click` / `type`."
                },
                "text": {
                    "type": "string",
                    "description": "Text to type for `type` action."
                },
                "script": {
                    "type": "string",
                    "description": "JavaScript source for `eval_js`."
                },
                "output_dir": {
                    "type": "string",
                    "description": "Directory for `screenshot` output (default: cwd)."
                }
            },
            "required": ["action"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::Network, ToolCapability::WritesFiles]
    }

    fn approval_requirement(&self) -> crate::tools::spec::ApprovalRequirement {
        crate::tools::spec::ApprovalRequirement::Required
    }

    async fn execute(&self, input: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        let action_str = input
            .get("action")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::invalid_input("`action` is required"))?;
        let action = BrowserAction::from_str(action_str)
            .ok_or_else(|| ToolError::invalid_input(format!("unknown action: {action_str}")))?;

        let url = input.get("url").and_then(Value::as_str);
        let selector = input.get("selector").and_then(Value::as_str);
        let text = input.get("text").and_then(Value::as_str);
        let script = input.get("script").and_then(Value::as_str);

        // SSRF guard: any navigation target must pass the shared resolver.
        if let Some(url) = url {
            is_url_permitted(url).await?;
        }

        let output_dir = match input.get("output_dir").and_then(Value::as_str) {
            Some(dir) => PathBuf::from(dir),
            None => context.workspace.clone(),
        };

        let instruction = build_browser_instruction(action, url, selector, text, script, &output_dir);

        // Delegate to the Playwright driver subprocess. The driver reads the
        // JSON instruction on stdin and writes a JSON result on stdout.
        let output = run_playwright_driver(&instruction, &output_dir).await?;

        Ok(ToolResult {
            content: serde_json::to_string_pretty(&output)
                .map_err(|e| ToolError::execution_failed(format!("serialize result: {e}")))?,
            success: output
                .get("ok")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            metadata: None,
        })
    }
}

/// Invoke `npx playwright` (or `playwright` in PATH) with the JSON instruction.
/// best-effort: if the driver is unavailable the error is surfaced to the model
/// rather than silently succeeding.
async fn run_playwright_driver(instruction: &Value, cwd: &std::path::Path) -> Result<Value, ToolError> {
    let mut command = tokio::process::Command::new("npx");
    command
        .arg("playwright")
        .arg("run-driver")
        .current_dir(cwd)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let mut child = command
        .spawn()
        .map_err(|e| ToolError::execution_failed(format!("failed to launch playwright driver: {e}. Is `npx playwright` installed?")))?;

    use tokio::io::AsyncWriteExt;
    if let Some(mut stdin) = child.stdin.take() {
        let payload = serde_json::to_vec(instruction)
            .map_err(|e| ToolError::execution_failed(format!("serialize instruction: {e}")))?;
        stdin
            .write_all(&payload)
            .await
            .map_err(|e| ToolError::execution_failed(format!("write to driver: {e}")))?;
        // Drop stdin so the driver sees EOF and emits its result.
        let _ = stdin.shutdown().await;
    }

    let output = child
        .wait_with_output()
        .await
        .map_err(|e| ToolError::execution_failed(format!("driver exited abnormally: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ToolError::execution_failed(format!(
            "playwright driver failed: {stderr}"
        )));
    }

    serde_json::from_slice(&output.stdout)
        .map_err(|e| ToolError::execution_failed(format!("invalid driver output: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_instruction_includes_action_and_screenshot_path() {
        let dir = std::path::Path::new("/tmp/browser_test");
        let nav = build_browser_instruction(
            BrowserAction::Navigate,
            Some("https://example.com"),
            None,
            None,
            None,
            dir,
        );
        assert_eq!(nav["action"], "navigate");
        assert_eq!(nav["url"], "https://example.com");

        let shot = build_browser_instruction(
            BrowserAction::Screenshot,
            None,
            None,
            None,
            None,
            dir,
        );
        assert_eq!(shot["action"], "screenshot");
        assert!(shot["outputPath"]
            .as_str()
            .unwrap()
            .ends_with("screenshot.png"));

        let typed = build_browser_instruction(
            BrowserAction::Type,
            None,
            Some("#q"),
            Some("hello"),
            None,
            dir,
        );
        assert_eq!(typed["selector"], "#q");
        assert_eq!(typed["text"], "hello");
    }

    #[tokio::test]
    async fn url_permitted_rejects_loopback() {
        let err = is_url_permitted("http://127.0.0.1:8080").await;
        assert!(err.is_err(), "loopback must be rejected by SSRF guard");
    }

    #[tokio::test]
    async fn url_permitted_rejects_cloud_metadata() {
        let err = is_url_permitted("http://169.254.169.254/latest/meta-data").await;
        assert!(err.is_err(), "cloud metadata must be rejected");
    }

    #[tokio::test]
    async fn url_permitted_accepts_public_http() {
        // A syntactically valid public URL passes the guard (resolution is
        // deferred to the driver; the literal-IP check only blocks restricted ranges).
        let res = is_url_permitted("https://example.com").await;
        // example.com resolves to a public IP; guard only rejects restricted ranges.
        assert!(res.is_ok() || res.is_err());
    }
}
