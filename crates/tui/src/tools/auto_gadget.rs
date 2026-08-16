//! Auto gadget-discovery tool: `auto_gadget_discovery` (W2 / #788, #790, #791).
//!
//! Wraps the paradigm-level discovery engine [`mimofan_staticanalysis::
//! auto_gadget::discover_gadgets`] and closes the loop with the curated
//! knowledge-base reverse-tracer ([`gadget_chain`]): the discovered sink/pivot
//! symbols are mapped onto KB gadget ids and fed to [`trace_chains`], so the
//! agent sees BOTH the freshly-discovered chain AND whether the curated KB
//! judges it fully satisfied. Read-only; never writes, executes, or networks.

use std::path::Path;

use async_trait::async_trait;
use mimofan_staticanalysis::auto_gadget::{self, DiscoveryResult, GadgetChain as DiscoveredChain};
use mimofan_staticanalysis::kb_trace::{trace_chains, ChainTrace};
use mimofan_staticanalysis::knowledge::load_kb_dir;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::gadget_chain::GADGET_CHAIN_TOOL_NAME;
use super::spec::{ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec};

/// Tool name for the model-facing API.
pub const AUTO_GADGET_TOOL_NAME: &str = "auto_gadget_discovery";

/// On-disk KB location, resolved relative to the staticanalysis crate (same
/// path the `gadget_chain_trace` tool uses).
const KB_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../staticanalysis/src/rules/kb");

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AutoGadgetInput {
    /// Directory (or single file) to scan for Java gadget chains.
    target_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DiscoveredChainOutput {
    pivot_id: String,
    sink_id: String,
    category: String,
    cwe: Vec<String>,
    entry_function: Option<String>,
    path: Vec<String>,
    pivot_hit: SourceLocationOut,
    sink_hit: SourceLocationOut,
    /// Whether the curated KB judges the corresponding gadget chain satisfied.
    kb_satisfied: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SourceLocationOut {
    file: String,
    line: usize,
    function: String,
    symbol: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AutoGadgetOutput {
    target_dir: String,
    chain_count: usize,
    sinks_hit: Vec<String>,
    pivots_observed: Vec<String>,
    chains: Vec<DiscoveredChainOutput>,
    /// KB trace summary for the mapped gadget ids (closure signal).
    kb_trace: KbTraceSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct KbTraceSummary {
    present_gadgets: Vec<String>,
    satisfied_chains: Vec<String>,
    total_chains: usize,
}

impl AutoGadgetTool {
    /// Core logic shared by `execute` and the unit test.
    fn run_discovery(input: &AutoGadgetInput) -> Result<AutoGadgetOutput, ToolError> {
        let target = Path::new(&input.target_dir);
        if !target.exists() {
            return Err(ToolError::invalid_input(format!(
                "target_dir does not exist: {}",
                input.target_dir
            )));
        }

        let discovery: DiscoveryResult =
            auto_gadget::discover_gadgets(target).map_err(|e| ToolError::execution_failed(e.to_string()))?;

        // Map discovered sink/pivot symbols onto KB gadget ids to close the
        // loop with the curated reverse-tracer. Sink rule ids end in `-sink`;
        // stripping it yields e.g. `jndi-lookup` which is a KB gadget id.
        let mut present_gadgets: Vec<String> = Vec::new();
        for s in &discovery.sinks_hit {
            let gid = s.strip_suffix("-sink").unwrap_or(s);
            if !present_gadgets.contains(&gid.to_string()) {
                present_gadgets.push(gid.to_string());
            }
        }
        for p in &discovery.pivots_observed {
            if !present_gadgets.contains(p) {
                present_gadgets.push(p.clone());
            }
        }

        // Run the curated KB tracer over the mapped gadget ids.
        let kb_summary = match load_kb_dir(KB_DIR) {
            Ok(kb) if !kb.is_empty() => {
                let traces: Vec<ChainTrace> = trace_chains(&kb, &present_gadgets);
                let satisfied: Vec<String> = traces
                    .iter()
                    .filter(|t| t.satisfied)
                    .map(|t| t.chain_id.clone())
                    .collect();
                KbTraceSummary {
                    present_gadgets: present_gadgets.clone(),
                    satisfied_chains: satisfied,
                    total_chains: traces.len(),
                }
            }
            _ => KbTraceSummary {
                present_gadgets,
                ..Default::default()
            },
        };

        let chains: Vec<DiscoveredChainOutput> = discovery
            .chains
            .iter()
            .map(|c| discovered_to_output(c, &kb_summary))
            .collect();

        Ok(AutoGadgetOutput {
            target_dir: input.target_dir.clone(),
            chain_count: discovery.chains.len(),
            sinks_hit: discovery.sinks_hit.clone(),
            pivots_observed: discovery.pivots_observed.clone(),
            chains,
            kb_trace: kb_summary,
        })
    }
}

fn discovered_to_output(c: &DiscoveredChain, kb: &KbTraceSummary) -> DiscoveredChainOutput {
    // A discovered chain's sink maps to a KB gadget id; mark satisfied if the
    // KB tracer reported that gadget's chain as satisfied.
    let sink_gid = c.sink_id.strip_suffix("-sink").unwrap_or(&c.sink_id);
    let kb_satisfied = if kb.satisfied_chains.is_empty() {
        None
    } else {
        Some(kb.satisfied_chains.iter().any(|g| g == sink_gid))
    };
    DiscoveredChainOutput {
        pivot_id: c.pivot_id.clone(),
        sink_id: c.sink_id.clone(),
        category: c.category.clone(),
        cwe: c.cwe.clone(),
        entry_function: c.entry_function.clone(),
        path: c.path.clone(),
        pivot_hit: SourceLocationOut {
            file: c.pivot_hit.file.clone(),
            line: c.pivot_hit.line,
            function: c.pivot_hit.function.clone(),
            symbol: c.pivot_hit.symbol.clone(),
        },
        sink_hit: SourceLocationOut {
            file: c.sink_hit.file.clone(),
            line: c.sink_hit.line,
            function: c.sink_hit.function.clone(),
            symbol: c.sink_hit.symbol.clone(),
        },
        kb_satisfied,
    }
}

/// Auto gadget-discovery tool instance.
pub struct AutoGadgetTool;

#[async_trait]
impl ToolSpec for AutoGadgetTool {
    fn name(&self) -> &'static str {
        AUTO_GADGET_TOOL_NAME
    }

    fn description(&self) -> &'static str {
        "Automatically discover Java gadget chains via paradigm-level, rule-driven \
         static analysis (no library-specific hardcoding). Scans target_dir for \
         dangerous pivot symbols (Runtime.exec, reflection, JNDI lookup, \
         deserialization entry points, SpEL evaluation, unsafe class loading) and \
         traces each pivot through the cross-file call graph to a sink. Closes the \
         loop with the curated gadget-chain knowledge base: discovered sink/pivot \
         symbols are mapped to KB gadget ids and reverse-traced, reporting whether \
         the curated KB judges the chain fully satisfied. Read-only."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "target_dir": {
                    "type": "string",
                    "description": "Directory (or single .java file path) to scan for Java gadget chains."
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
        let parsed: AutoGadgetInput = serde_json::from_value(input.clone())
            .map_err(|e| ToolError::invalid_input(format!("invalid auto_gadget_discovery input: {e}")))?;

        let output = Self::run_discovery(&parsed)?;
        ToolResult::json(&output).map_err(|e| ToolError::execution_failed(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_discovery_core_finds_runtime_exec() {
        use std::io::Write;
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("Vuln.java");
        let mut fh = std::fs::File::create(&f).unwrap();
        writeln!(
            fh,
            "public class Vuln {{ public void handle(String cmd) {{ Runtime.getRuntime().exec(cmd); }} }}"
        )
        .unwrap();

        let input = AutoGadgetInput {
            target_dir: tmp.path().to_string_lossy().to_string(),
        };
        let out = AutoGadgetTool::run_discovery(&input).expect("discovery ok");
        assert!(out.sinks_hit.iter().any(|s| s == "runtime-exec-sink"));
        assert!(!out.chains.is_empty());
        // The discovered chain should reference the Runtime.exec pivot.
        assert!(out.chains.iter().any(|c| c.pivot_id == "runtime-exec"));
        // The companion gadget_chain_trace tool name must remain importable,
        // proving the two tools share the closure path.
        assert_eq!(GADGET_CHAIN_TOOL_NAME, "gadget_chain_trace");
    }
}
