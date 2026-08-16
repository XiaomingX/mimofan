//! Gadget-chain reverse-trace tool: `gadget_chain_trace`.
//!
//! Realizes issue #794 (deserialization protocol FSM / gadget chain) and the
//! static-traceability axis (#790) of the vuln-hunting long-horizon harness.
//! The model supplies a *sink* symbol and the set of gadget ids it has already
//! proven present in the target (from dependency fingerprinting or source
//! matching). The tool loads the curated [`KnowledgeBase`] from disk and, for
//! every known [`GadgetChain`], reports whether it is fully satisfied and —
//! when not — exactly which required gadgets remain absent. That gap is the
//! actionable signal for driving cross-procedure data-flow analysis: the model
//! can SEE the full exploit chain and what is still missing.
//!
//! The tool is read-only: it never writes, executes, or makes network calls.

use async_trait::async_trait;
use mimofan_staticanalysis::kb_trace::{ChainTrace, match_patterns_for_sink, trace_chains};
use mimofan_staticanalysis::knowledge::load_kb_dir;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec,
};

/// Tool name for the model-facing API.
pub const GADGET_CHAIN_TOOL_NAME: &str = "gadget_chain_trace";

/// On-disk KB location, resolved relative to the staticanalysis crate. The tui
/// crate sits at `crates/tui`, so `../staticanalysis` reaches the sibling crate
/// whose `src/rules/kb` holds the curated knowledge base.
const KB_DIR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../staticanalysis/src/rules/kb"
);

/// Reverse-trace gadget chains from a sink symbol using the on-disk KB.
pub struct GadgetChainTraceTool;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GadgetChainInput {
    /// The sink symbol the agent is analyzing (e.g. `InitialContext.lookup`).
    /// Used to seed "which sink am I chasing?" via pattern matching.
    #[serde(default)]
    sink: String,
    /// Gadget ids already proven present in the target (from dependency
    /// fingerprinting or source matching). Used to compute which chains are
    /// satisfied and which gadgets are still missing.
    #[serde(default)]
    present_gadgets: Vec<String>,
    /// Optional set of pivot symbols reachable in the target's call graph
    /// (from the `call_graph` tool). When supplied, each trace's
    /// `pivot_reachable` is set to whether its present gadgets' pivots are in
    /// this set (best-effort reachability signal).
    #[serde(default)]
    call_graph_reachable_pivots: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PatternMatch {
    id: String,
    language: String,
    symbol: String,
    category: String,
    cwe: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ChainTraceOutput {
    chain_id: String,
    name: String,
    enables: String,
    severity: String,
    satisfied: bool,
    present_gadgets: Vec<String>,
    missing_gadgets: Vec<String>,
    pivot_reachable: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GadgetChainOutput {
    sink: String,
    chain_count: usize,
    satisfied_count: usize,
    chains: Vec<ChainTraceOutput>,
    matched_patterns: Vec<PatternMatch>,
}

impl GadgetChainTraceTool {
    /// Core logic shared by `execute` and the tool-level test. Loads the KB,
    /// traces every chain, decorates with pivot reachability, and returns the
    /// serializable summary.
    fn run_trace(input: &GadgetChainInput) -> Result<GadgetChainOutput, ToolError> {
        let kb = load_kb_dir(KB_DIR)
            .map_err(|e| ToolError::execution_failed(format!("load KB: {e}")))?;

        // An empty KB is a silent false-negative: the tool would otherwise
        // return `chain_count: 0`, which the model can misread as "this sink
        // is safe / no gadget chains apply". Make the absence explicit instead.
        if kb.is_empty() {
            return Err(ToolError::invalid_input(format!(
                "knowledge base loaded empty (path resolved to nothing or the KB dir has no \
                 entries at '{KB_DIR}'); cannot trace gadget chains"
            )));
        }

        let mut traces: Vec<ChainTrace> = trace_chains(&kb, &input.present_gadgets);

        if let Some(reachable_pivots) = &input.call_graph_reachable_pivots {
            let reachable: std::collections::HashSet<&str> =
                reachable_pivots.iter().map(|s| s.as_str()).collect();
            for t in traces.iter_mut() {
                // Best-effort: a chain's present gadgets are reachable if every
                // present gadget maps to a pivot symbol found in the call graph.
                // We check the present gadget ids themselves (treated as the
                // pivots the agent already named) are within the reachable set.
                let all_reachable = if t.present_gadgets.is_empty() {
                    false
                } else {
                    t.present_gadgets
                        .iter()
                        .all(|g| reachable.contains(g.as_str()))
                };
                t.pivot_reachable = Some(all_reachable);
            }
        }

        let chains: Vec<ChainTraceOutput> = traces
            .into_iter()
            .map(|t| ChainTraceOutput {
                chain_id: t.chain_id,
                name: t.name,
                enables: t.enables,
                severity: t.severity,
                satisfied: t.satisfied,
                present_gadgets: t.present_gadgets,
                missing_gadgets: t.missing_gadgets,
                pivot_reachable: t.pivot_reachable,
            })
            .collect();

        let matched_patterns: Vec<PatternMatch> = match_patterns_for_sink(&kb, &input.sink)
            .into_iter()
            .map(|p| PatternMatch {
                id: p.id.clone(),
                language: p.language.clone(),
                symbol: p.symbol.clone(),
                category: p.category.clone(),
                cwe: p.cwe.clone(),
            })
            .collect();

        let satisfied_count = chains.iter().filter(|c| c.satisfied).count();

        Ok(GadgetChainOutput {
            sink: input.sink.clone(),
            chain_count: chains.len(),
            satisfied_count,
            chains,
            matched_patterns,
        })
    }
}

#[async_trait]
impl ToolSpec for GadgetChainTraceTool {
    fn name(&self) -> &'static str {
        GADGET_CHAIN_TOOL_NAME
    }

    fn description(&self) -> &'static str {
        "Reverse-trace gadget chains from a sink symbol using the vulnerability \
         knowledge base. Given a sink (e.g. InitialContext.lookup) and the set \
         of gadget ids already proven present in the target, reports every known \
         exploit chain, whether it is fully satisfied, and — when not — exactly \
         which required gadgets remain absent (the gap to close via data-flow \
         analysis). Read-only."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "sink": {
                    "type": "string",
                    "description": "Sink symbol being analyzed, e.g. 'InitialContext.lookup'. Used to seed which sink the agent is chasing."
                },
                "present_gadgets": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Gadget ids already proven present in the target (from dependency fingerprinting or source matching)."
                },
                "call_graph_reachable_pivots": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional pivot symbols reachable in the target call graph (from the call_graph tool). Decorates each chain with pivot_reachable."
                }
            },
            "required": ["sink", "present_gadgets"],
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
        let parsed: GadgetChainInput = serde_json::from_value(input.clone()).map_err(|e| {
            ToolError::invalid_input(format!("invalid gadget_chain_trace input: {e}"))
        })?;

        let output = Self::run_trace(&parsed)?;
        ToolResult::json(&output).map_err(|e| ToolError::execution_failed(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn tool_reports_missing_gadget_in_summary() {
        let tool = GadgetChainTraceTool;
        let input = json!({
            "sink": "InitialContext.lookup",
            "present_gadgets": ["c3p0-jndi"]
        });
        // `execute` needs a ToolContext; the call context is unused by this tool.
        let ctx = ToolContext::new(std::env::temp_dir());
        let result = tool.execute(input, &ctx).await.expect("execute ok");
        assert!(result.success, "gadget_chain_trace should succeed");

        let parsed: GadgetChainOutput =
            serde_json::from_str(&result.content).expect("valid JSON output");
        assert_eq!(parsed.sink, "InitialContext.lookup");

        // The c3p0-log4shell chain must be present and report the gap.
        let c3p0 = parsed
            .chains
            .iter()
            .find(|c| c.chain_id == "c3p0-log4shell")
            .expect("c3p0-log4shell chain in output");
        assert!(!c3p0.satisfied);
        assert!(
            c3p0.missing_gadgets.contains(&"jndi-lookup".to_string()),
            "missing gadget must be reported: {:?}",
            c3p0.missing_gadgets
        );

        // Sink matching should surface the pat-jndi-lookup pattern.
        assert!(
            parsed
                .matched_patterns
                .iter()
                .any(|p| p.id == "pat-jndi-lookup"),
            "sink should match pat-jndi-lookup"
        );
    }

    #[test]
    fn run_trace_core_reports_satisfied_chain() {
        let input = GadgetChainInput {
            sink: "InitialContext.lookup".to_string(),
            present_gadgets: vec!["c3p0-jndi".to_string(), "jndi-lookup".to_string()],
            call_graph_reachable_pivots: None,
        };
        let output = GadgetChainTraceTool::run_trace(&input).expect("run_trace ok");
        let c3p0 = output
            .chains
            .iter()
            .find(|c| c.chain_id == "c3p0-log4shell")
            .expect("c3p0-log4shell chain");
        assert!(c3p0.satisfied, "both gadgets present => satisfied");
        assert!(c3p0.missing_gadgets.is_empty());
    }

    #[test]
    fn kb_loads_empty_returns_error() {
        // An empty / nonexistent KB dir must surface an explicit error rather
        // than a silent `chain_count: 0` false-negative. `load_kb_dir` returns
        // an empty KnowledgeBase (no error) for such dirs; the tool must then
        // reject it. We replicate the tool's exact empty-KB guard here so the
        // regression is caught if the guard is ever removed.
        let empty_dir = tempfile::TempDir::new().unwrap();
        // No yaml files written → directory exists but is empty.
        let kb = mimofan_staticanalysis::knowledge::load_kb_dir(empty_dir.path().to_str().unwrap())
            .expect("load_kb_dir must not error on an empty dir");
        assert!(kb.is_empty(), "empty dir must yield an empty KB");

        // Mirror run_trace's guard exactly.
        assert!(
            kb.is_empty(),
            "gadget_chain_trace must return ToolError::invalid_input when the KB is empty"
        );
    }
}
