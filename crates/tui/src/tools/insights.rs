//! Usage / cost / tool analytics (issue #744).
//!
//! Provides a pure, dependency-free aggregation engine over [`UsageRecord`]
//! events, grouped by session / tool / model, with Top-N rankings and a
//! human-readable report. It does NOT mutate the existing cost-accrual path
//! (`crate::cost_status`); callers feed records in and read aggregated views.
//!
//! The aggregation functions are unit-tested independently of any live
//! telemetry source.

use std::collections::BTreeMap;

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::models::Usage;
use crate::pricing::calculate_turn_cost_estimate_from_usage;
use crate::tools::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec,
};

/// A single observed LLM usage event, tagged with the dimensions we analyze.
#[derive(Debug, Clone, Default)]
pub struct UsageRecord {
    pub session_id: String,
    pub tool: String,
    pub model: String,
    pub usage: Usage,
}

/// Accumulated metrics for one dimension key (tool / session / model).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DimensionStats {
    pub calls: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// Cost in USD (sum of per-event estimates).
    pub usd: f64,
}

impl DimensionStats {
    fn add(&mut self, model: &str, usage: &Usage) {
        self.calls += 1;
        self.input_tokens += u64::from(usage.input_tokens);
        self.output_tokens += u64::from(usage.output_tokens);
        if let Some(cost) = calculate_turn_cost_estimate_from_usage(model, usage) {
            self.usd += cost.usd;
        }
    }

    /// Total tokens across this dimension.
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens + self.output_tokens
    }
}

/// Aggregates usage records across sessions / tools / models.
#[derive(Debug, Clone, Default)]
pub struct InsightsAggregator {
    by_tool: BTreeMap<String, DimensionStats>,
    by_session: BTreeMap<String, DimensionStats>,
    by_model: BTreeMap<String, DimensionStats>,
    totals: DimensionStats,
}

impl InsightsAggregator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&mut self, record: UsageRecord) {
        let UsageRecord {
            session_id,
            tool,
            model,
            usage,
        } = &record;
        self.by_tool
            .entry(tool.clone())
            .or_default()
            .add(model, usage);
        self.by_session
            .entry(session_id.clone())
            .or_default()
            .add(model, usage);
        self.by_model
            .entry(model.clone())
            .or_default()
            .add(model, usage);
        self.totals.add(model, usage);
    }

    /// Tool-dimension stats, sorted by cost descending.
    pub fn by_tool(&self) -> Vec<(String, DimensionStats)> {
        sort_by_cost(&self.by_tool)
    }

    /// Session-dimension stats, sorted by cost descending.
    pub fn by_session(&self) -> Vec<(String, DimensionStats)> {
        sort_by_cost(&self.by_session)
    }

    /// Model-dimension stats, sorted by cost descending.
    pub fn by_model(&self) -> Vec<(String, DimensionStats)> {
        sort_by_cost(&self.by_model)
    }

    pub fn totals(&self) -> &DimensionStats {
        &self.totals
    }

    /// Top-N tools by cost (used for the report's "most expensive tool" view).
    pub fn top_tools(&self, n: usize) -> Vec<(String, DimensionStats)> {
        self.by_tool().into_iter().take(n).collect()
    }

    /// Render a human-readable report. Returns an empty string when there are
    /// no records (so callers can detect "nothing to show").
    pub fn render_report(&self) -> String {
        if self.totals.calls == 0 {
            return String::new();
        }
        let mut out = String::new();
        let t = &self.totals;
        out.push_str("=== mimofan insights ===\n");
        out.push_str(&format!(
            "total: {} calls, {} in / {} out tokens, ${:.4} USD\n",
            t.calls, t.input_tokens, t.output_tokens, t.usd
        ));

        out.push_str("\nTop tools by cost:\n");
        for (i, (tool, s)) in self.top_tools(5).iter().enumerate() {
            out.push_str(&format!(
                "  {}. {} — {} calls, {} tok, ${:.4}\n",
                i + 1,
                tool,
                s.calls,
                s.total_tokens(),
                s.usd
            ));
        }

        out.push_str("\nBy model:\n");
        for (model, s) in self.by_model() {
            out.push_str(&format!("  {} — {} calls, ${:.4}\n", model, s.calls, s.usd));
        }
        out
    }

    /// JSON view for machine consumers / downstream panels.
    pub fn to_json(&self) -> Value {
        json!({
            "totals": stats_to_json(self.totals()),
            "byTool": map_to_json(&self.by_tool),
            "bySession": map_to_json(&self.by_session),
            "byModel": map_to_json(&self.by_model),
        })
    }
}

fn sort_by_cost(map: &BTreeMap<String, DimensionStats>) -> Vec<(String, DimensionStats)> {
    let mut items: Vec<(String, DimensionStats)> =
        map.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    items.sort_by(|a, b| {
        b.1.usd
            .partial_cmp(&a.1.usd)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    items
}

fn stats_to_json(s: &DimensionStats) -> Value {
    json!({
        "calls": s.calls,
        "inputTokens": s.input_tokens,
        "outputTokens": s.output_tokens,
        "totalTokens": s.total_tokens(),
        "usd": s.usd,
    })
}

fn map_to_json(map: &BTreeMap<String, DimensionStats>) -> Value {
    let mut obj = serde_json::Map::new();
    for (k, v) in map {
        obj.insert(k.clone(), stats_to_json(v));
    }
    Value::Object(obj)
}

/// Tool wrapper so `/insights` (or a model) can compute analytics from a batch
/// of usage records without touching the live cost-accrual side-channel.
pub struct InsightsTool;

#[async_trait]
impl ToolSpec for InsightsTool {
    fn name(&self) -> &'static str {
        "insights"
    }

    fn description(&self) -> &'static str {
        "Aggregate LLM usage/cost/tool analytics from a batch of usage records. Input: `records` (array of {session_id, tool, model, input_tokens, output_tokens}). Returns a cost-ranked report by tool/session/model with Top-N. Purely local; does not affect the running cost tally."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "records": {
                    "type": "array",
                    "description": "Usage records to aggregate.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "session_id": { "type": "string" },
                            "tool": { "type": "string" },
                            "model": { "type": "string" },
                            "input_tokens": { "type": "integer" },
                            "output_tokens": { "type": "integer" }
                        },
                        "required": ["tool", "model", "input_tokens", "output_tokens"]
                    }
                },
                "format": {
                    "type": "string",
                    "enum": ["text", "json"],
                    "description": "Output format (default text)."
                }
            },
            "required": ["records"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    async fn execute(&self, input: Value, _context: &ToolContext) -> Result<ToolResult, ToolError> {
        let records = input
            .get("records")
            .and_then(Value::as_array)
            .ok_or_else(|| ToolError::invalid_input("`records` array is required"))?;

        let mut agg = InsightsAggregator::new();
        for r in records {
            let tool = r
                .get("tool")
                .and_then(Value::as_str)
                .ok_or_else(|| ToolError::invalid_input("each record needs `tool`"))?
                .to_string();
            let model = r
                .get("model")
                .and_then(Value::as_str)
                .ok_or_else(|| ToolError::invalid_input("each record needs `model`"))?
                .to_string();
            let input_tokens = r
                .get("input_tokens")
                .and_then(Value::as_u64)
                .ok_or_else(|| ToolError::invalid_input("each record needs `input_tokens`"))?
                as u32;
            let output_tokens = r
                .get("output_tokens")
                .and_then(Value::as_u64)
                .ok_or_else(|| ToolError::invalid_input("each record needs `output_tokens`"))?
                as u32;
            let session_id = r
                .get("session_id")
                .and_then(Value::as_str)
                .unwrap_or("default")
                .to_string();
            agg.record(UsageRecord {
                session_id,
                tool,
                model,
                usage: Usage {
                    input_tokens,
                    output_tokens,
                    ..Default::default()
                },
            });
        }

        let format = input
            .get("format")
            .and_then(Value::as_str)
            .unwrap_or("text");
        let content = if format == "json" {
            serde_json::to_string_pretty(&agg.to_json())
                .map_err(|e| ToolError::execution_failed(e.to_string()))?
        } else {
            agg.render_report()
        };

        Ok(ToolResult {
            content,
            success: true,
            metadata: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Usage;

    fn rec(session: &str, tool: &str, model: &str, in_t: u32, out_t: u32) -> UsageRecord {
        UsageRecord {
            session_id: session.to_string(),
            tool: tool.to_string(),
            model: model.to_string(),
            usage: Usage {
                input_tokens: in_t,
                output_tokens: out_t,
                ..Default::default()
            },
        }
    }

    #[test]
    fn aggregates_by_tool_correctly() {
        let mut agg = InsightsAggregator::new();
        agg.record(rec("s1", "read_file", "deepseek", 100, 10));
        agg.record(rec("s1", "read_file", "deepseek", 200, 20));
        agg.record(rec("s2", "exec_shell", "deepseek", 50, 5));

        let by_tool = agg.by_tool();
        assert_eq!(by_tool.len(), 2);
        // read_file has higher cost → first
        assert_eq!(by_tool[0].0, "read_file");
        assert_eq!(by_tool[0].1.calls, 2);
        assert_eq!(by_tool[0].1.input_tokens, 300);
        assert_eq!(by_tool[0].1.output_tokens, 30);
        assert_eq!(by_tool[1].0, "exec_shell");
        assert_eq!(by_tool[1].1.calls, 1);
    }

    #[test]
    fn totals_accumulate_all_dimensions() {
        let mut agg = InsightsAggregator::new();
        agg.record(rec("s1", "a", "m1", 100, 10));
        agg.record(rec("s2", "b", "m2", 40, 5));
        let t = agg.totals();
        assert_eq!(t.calls, 2);
        assert_eq!(t.input_tokens, 140);
        assert_eq!(t.output_tokens, 15);
        assert_eq!(agg.by_session().len(), 2);
        assert_eq!(agg.by_model().len(), 2);
    }

    #[test]
    fn render_report_empty_when_no_records() {
        let agg = InsightsAggregator::new();
        assert!(agg.render_report().is_empty());
    }

    #[test]
    fn render_report_nonempty_with_records() {
        let mut agg = InsightsAggregator::new();
        agg.record(rec("s1", "read_file", "deepseek", 100, 10));
        let report = agg.render_report();
        assert!(report.contains("mimofan insights"));
        assert!(report.contains("read_file"));
    }

    #[test]
    fn top_tools_respects_limit() {
        let mut agg = InsightsAggregator::new();
        for i in 0..7 {
            agg.record(rec("s", &format!("tool{i}"), "deepseek", 10, 1));
        }
        assert_eq!(agg.top_tools(3).len(), 3);
    }
}
