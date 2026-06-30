//! `notify` tool — model-callable desktop notification (#1322).
//!
//! Routes through the existing `tui::notifications` infrastructure (OSC 9
//! for known capable terminals, BEL fallback on macOS / Linux, `MessageBeep`
//! on Windows when explicitly opted in). The model decides when to fire —
//! the tool is intended for "long task done, come back" beats and
//! sub-agent-completion pings, not chatter.
//!
//! Auto-suppresses when `[notifications].method = "off"`. Output messages
//! are length-capped so a runaway model can't paint a paragraph into the
//! terminal title bar.

use async_trait::async_trait;
use serde_json::{Value, json};

use super::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec,
    optional_str, required_str,
};
use crate::tui::notifications::{Method, notify_done};

/// Maximum chars passed through for the title — keeps the OSC 9 escape
/// reasonable on terminals that wrap long titles awkwardly.
const NOTIFY_TITLE_CAP: usize = 80;
/// Maximum chars passed through for the body. Most receivers truncate
/// past ~120, so 200 leaves headroom while still bounded.
const NOTIFY_BODY_CAP: usize = 200;

/// Tool that fires a single desktop notification.
pub struct NotifyTool;

#[async_trait]
impl ToolSpec for NotifyTool {
    fn name(&self) -> &'static str {
        "notify"
    }

    fn description(&self) -> &'static str {
        "Fire a single desktop notification (OSC 9 / terminal bell). Use \
         sparingly — only when a long-running task completes, when a turn \
         was waiting on a remote operation that just finished, or when \
         the user genuinely needs to come back to the terminal. Pass a \
         short `title` and an optional `body`. Do NOT use this for \
         routine progress updates, conversational acknowledgements, or \
         confirmation that the model is alive — that's noise. The user \
         can disable notifications entirely via \
         `[notifications].method = \"off\"` in `~/.deepseek/config.toml`; \
         when disabled this tool is a silent no-op."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "title": {
                    "type": "string",
                    "description": "Short notification title (≤ 80 chars after truncation). Required."
                },
                "body": {
                    "type": "string",
                    "description": "Optional longer body (≤ 200 chars after truncation)."
                }
            },
            "required": ["title"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        // No filesystem or shell side effects; the only output is a single
        // terminal-escape write to stdout. Mark as ReadOnly so the
        // approval-requirement default is `Auto` and the tool routes
        // through without prompting.
        vec![ToolCapability::ReadOnly]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    async fn execute(&self, input: Value, _ctx: &ToolContext) -> Result<ToolResult, ToolError> {
        let title_raw = required_str(&input, "title")?;
        let body_raw = optional_str(&input, "body").unwrap_or("");

        // Char-bounded truncation (not byte-bounded) so we don't slice
        // through a multi-byte sequence and emit invalid UTF-8 to the
        // terminal.
        let title: String = title_raw.chars().take(NOTIFY_TITLE_CAP).collect();
        let body: String = body_raw.chars().take(NOTIFY_BODY_CAP).collect();
        let title = title.trim();
        let body = body.trim();

        if title.is_empty() {
            return Err(ToolError::execution_failed("title must not be empty"));
        }

        let msg = if body.is_empty() {
            title.to_string()
        } else {
            format!("{title}: {body}")
        };

        let in_tmux = std::env::var("TMUX")
            .map(|v| !v.is_empty())
            .unwrap_or(false);

        // Threshold = 0 so the notification always fires; the model has
        // already decided this is the moment.
        notify_done(
            Method::Auto,
            in_tmux,
            &msg,
            std::time::Duration::ZERO,
            std::time::Duration::from_secs(1),
        );

        Ok(ToolResult::success(format!("notified: {title}")))
    }
}

#[cfg(test)]
mod tests {}
