//! Reverse gadget-chain tracing over the vulnerability knowledge base (T-12).
//!
//! Given a set of gadget ids *present* in a target (from dependency
//! fingerprinting or source matching) and the curated [`KnowledgeBase`], this
//! module computes, for every known [`GadgetChain`], whether it is fully
//! satisfied and — when not — exactly which required gadgets are still absent.
//! That gap is the actionable signal a vuln-hunting agent needs to drive
//! cross-procedure data-flow analysis (issue #790): it knows the sink it is
//! chasing (a `GadgetPattern`) and which pieces of the chain remain to be
//! proven reachable.
//!
//! Severity ordering follows the convention `critical > error > warning >
//! info`; chains are returned sorted by (severity rank, id) so the most
//! dangerous complete/near-complete chains surface first.

use std::collections::HashSet;

use crate::knowledge::{GadgetChain, GadgetPattern, KnowledgeBase};

/// Result of tracing one gadget chain against the target's present gadgets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainTrace {
    /// Chain id (e.g. `c3p0-log4shell`).
    pub chain_id: String,
    /// Human-readable chain name.
    pub name: String,
    /// Exploit class this chain enables (e.g. `jndi-injection`).
    pub enables: String,
    /// Severity string as stored in the KB (`critical`/`error`/`warning`/`info`).
    pub severity: String,
    /// True iff every required gadget is present in the target.
    pub satisfied: bool,
    /// Required gadgets that ARE present.
    pub present_gadgets: Vec<String>,
    /// Required gadgets that are ABSENT — the gap the agent must close.
    pub missing_gadgets: Vec<String>,
    /// Whether the chain's pivot gadget is reachable in the supplied call
    /// graph. `None` when no call-graph reachability set was provided.
    pub pivot_reachable: Option<bool>,
}

impl ChainTrace {
    /// Rank used for sorting: lower = more severe.
    fn severity_rank(&self) -> u8 {
        match self.severity.as_str() {
            "critical" => 0,
            "high" => 1,
            "error" => 2,
            "medium" => 3,
            "warning" => 4,
            "low" => 5,
            "info" => 6,
            _ => 7,
        }
    }
}

/// Compute a [`ChainTrace`] for a single chain.
fn trace_one(chain: &GadgetChain, present: &HashSet<&str>) -> ChainTrace {
    let mut present_gadgets: Vec<String> = chain
        .requires
        .iter()
        .filter(|r| present.contains(r.as_str()))
        .cloned()
        .collect();
    let mut missing_gadgets: Vec<String> = chain
        .requires
        .iter()
        .filter(|r| !present.contains(r.as_str()))
        .cloned()
        .collect();
    present_gadgets.sort();
    missing_gadgets.sort();

    ChainTrace {
        chain_id: chain.id.clone(),
        name: chain.name.clone(),
        enables: chain.enables.clone(),
        severity: chain.severity.clone(),
        satisfied: missing_gadgets.is_empty(),
        present_gadgets,
        missing_gadgets,
        pivot_reachable: None,
    }
}

/// Trace every chain in the KB against the set of present gadget ids.
///
/// Returns one [`ChainTrace`] per chain, sorted by severity (most severe
/// first) then by chain id for stable ordering within a severity tier.
///
/// `present_gadgets` are the gadget ids known to exist in the target (from
/// dependency fingerprinting or source matching). They need not be a subset of
/// the KB's known gadget ids — unknown ids are simply ignored.
pub fn trace_chains(kb: &KnowledgeBase, present_gadgets: &[String]) -> Vec<ChainTrace> {
    let present: HashSet<&str> = present_gadgets.iter().map(|s| s.as_str()).collect();
    let mut traces: Vec<ChainTrace> = kb.chains.iter().map(|c| trace_one(c, &present)).collect();
    traces.sort_by(|a, b| {
        a.severity_rank()
            .cmp(&b.severity_rank())
            .then(a.chain_id.cmp(&b.chain_id))
    });
    traces
}

/// Return every [`GadgetPattern`] whose `symbol` equals or contains
/// `sink_symbol`. Used by the model-facing tool to seed "which sink am I
/// analyzing?" — e.g. `InitialContext.lookup` yields the `pat-jndi-lookup`
/// pattern.
pub fn match_patterns_for_sink<'a>(
    kb: &'a KnowledgeBase,
    sink_symbol: &str,
) -> Vec<&'a GadgetPattern> {
    kb.patterns
        .iter()
        .filter(|p| p.symbol == sink_symbol || p.symbol.contains(sink_symbol))
        .collect()
}
