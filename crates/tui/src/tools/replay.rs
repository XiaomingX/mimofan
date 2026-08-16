//! #850 — `EventStreamTool`: a read-only tool surface over the event stream.
//!
//! This implements [`ToolSpec`] so the structured log (see [`super::event_stream`])
//! can later be exposed to the model as a tool. Registration is intentionally
//! deferred — this module only provides the struct and constructor. It must not
//! be wired into `mod.rs`/`registry.rs` by this change.
//!
//! The tool is `ReadOnly`: it reports the configured log path (and optionally a
//! small summary) without appending or mutating anything.

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::tools::spec::{ToolContext, ToolError, ToolSpec};
use mimofan_tools::{ApprovalRequirement, ToolCapability, ToolResult};

use super::event_stream::EventReplay;

/// A read-only tool that surfaces the active structured-event log.
///
/// Construct it with the path events are (or will be) written to. `execute`
/// returns that path (and, when requested, aggregate counts) as JSON — it never
/// writes an event itself, so it stays safely `ReadOnly`.
pub struct EventStreamTool {
    /// Path of the JSON-Lines event log this tool points at.
    log_path: std::path::PathBuf,
}

impl EventStreamTool {
    /// Create the tool pointed at `log_path`.
    #[must_use]
    pub fn new(log_path: impl Into<std::path::PathBuf>) -> Self {
        Self {
            log_path: log_path.into(),
        }
    }

    /// The log path this tool reports.
    #[must_use]
    pub fn log_path(&self) -> &std::path::Path {
        &self.log_path
    }
}

#[async_trait]
impl ToolSpec for EventStreamTool {
    fn name(&self) -> &str {
        "event_stream"
    }

    fn description(&self) -> &str {
        "Report the path of mimofan's structured JSON-Lines event log and (optionally) a summary of recorded events. Read-only."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "summary": {
                    "type": "boolean",
                    "description": "If true, include aggregate event counts by kind. Defaults to false."
                }
            },
            "additionalProperties": false
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    async fn execute(&self, input: Value, _context: &ToolContext) -> Result<ToolResult, ToolError> {
        let want_summary = input
            .get("summary")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        let mut meta = serde_json::Map::new();
        meta.insert(
            "log_path".to_string(),
            Value::String(self.log_path.display().to_string()),
        );

        if want_summary {
            match EventReplay::new(&self.log_path).counts() {
                Ok(counts) => {
                    let by_kind: serde_json::Map<String, Value> = counts
                        .by_kind
                        .into_iter()
                        .map(|(k, v)| (k, json!(v)))
                        .collect();
                    meta.insert("total".to_string(), json!(counts.total));
                    meta.insert("by_kind".to_string(), Value::Object(by_kind));
                }
                Err(e) => {
                    // Replay is best-effort: report the path even if the file
                    // does not exist yet or cannot be read.
                    meta.insert("summary_error".to_string(), json!(e.to_string()));
                }
            }
        }

        Ok(ToolResult::success("Event stream log path").with_metadata(Value::Object(meta)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn tool_reports_path_and_is_read_only() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("events.jsonl");
        // Seed a real log so `summary: true` can compute counts.
        {
            let mut log = crate::tools::event_stream::EventLog::open(&path).unwrap();
            log.append(
                crate::tools::event_stream::EventKind::TurnStart,
                json!({"turn": 1}),
            )
            .unwrap();
            log.append(
                crate::tools::event_stream::EventKind::ToolCall,
                json!({"tool": "read_file"}),
            )
            .unwrap();
        }
        let tool = EventStreamTool::new(path.clone());

        assert!(tool.is_read_only());
        assert_eq!(tool.name(), "event_stream");
        assert!(tool.capabilities().contains(&ToolCapability::ReadOnly));

        let ctx = ToolContext::new(dir.path().to_path_buf());
        let result = tool.execute(json!({"summary": true}), &ctx).await.unwrap();
        assert!(result.success);
        let meta = result.metadata.unwrap();
        assert_eq!(meta["log_path"], path.display().to_string());
        assert_eq!(meta["total"], 2);
        assert_eq!(meta["by_kind"]["turn_start"], 1);
        assert_eq!(meta["by_kind"]["tool_call"], 1);

        // Without summary, no counts are attached — still reports the path.
        let result = tool.execute(json!({}), &ctx).await.unwrap();
        assert!(result.success);
        let meta = result.metadata.unwrap();
        assert_eq!(meta["log_path"], path.display().to_string());
        assert!(meta.get("total").is_none());
    }
}
