//! Inter-procedural taint propagation (T-6, #788/#790).
//!
//! The intra-procedural solver in `taint.rs` (`analyze`) propagates taint
//! *within a single compilation unit* and stops at call boundaries. This
//! module lifts that to **inter-procedural** analysis by combining three
//! already-existing, grammar-agnostic building blocks:
//!
//! 1. `taint::analyze` / `taint::analyze_with_seed` — per-unit intra-procedural
//!    solver (sources, propagators, sanitizers, field sensitivity, evidence
//!    chain). `analyze_with_seed` lets us inject taint that arrived from
//!    another file via a tainted *return value* (`ret_seed`).
//! 2. `callgraph::CallGraph` — a merged, cross-file call graph that records
//!    every call edge, including edges that cross file boundaries.
//! 3. `rules::RuleSet` — the declarative source/sink/propagator/sanitizer rules.
//!
//! # Algorithm
//!
//! A monotone **worklist / fixpoint** over the call graph:
//!
//! - **Pass 1 (seed):** run the intra-procedural solver on every unit. From
//!   each finding's evidence chain we read the *source* steps (steps whose
//!   `rule_id` matches a `SourceRule`). The enclosing function of such a step
//!   is the one that *produces* tainted data — its summary return-taint is
//!   set. This gives an initial per-function "returns taint" map keyed by the
//!   function name as it appears in the call graph.
//! - **Pass 2 (fixpoint):** for every call edge `caller -> callee`, if `callee`
//!   returns taint (per the summary) we seed the corresponding call *site*
//!   inside `caller`'s unit with a tainted return value (`ret_seed`) and
//!   re-solve `caller` intra-procedurally. The freshly produced findings update
//!   the summary, which may taint more callers — we repeat until no summary
//!   changes.
//! - The result is the union of every unit's findings *after* cross-file taint
//!   has been propagated to fixpoint. A source defined in one file now reaches
//!   a sink in another file, fixing the L2+ cross-file recall collapse
//!   reported in #843.
//!
//! This is deliberately **paradigmatic, not framework-specific**: it works on
//! whatever `ProgramFacts` a language frontend lowers and on whatever
//! `CallGraph` is built — Rust, Java, Spring Boot controllers, Fastjson
//! deserializers, etc. No rule or heuristic is hardcoded for a single target.

use std::collections::{HashMap, HashSet};

use crate::callgraph::{CallGraph, Function};
use crate::rules::RuleSet;
use crate::taint::{analyze, analyze_with_seed, ProgramFacts, TaintFinding, TaintTag};

/// Per-function return-taint summary, keyed by the function *name* as used in
/// the call graph (`Function.name`, the simple last-path-segment symbol).
#[derive(Debug, Clone, Default)]
pub struct FuncReturnSummary {
    /// Function symbol -> whether it returns tainted data (monotone).
    pub returns_taint: HashMap<String, bool>,
}

/// Inter-procedural analysis input: every compilation unit's facts, plus the
/// merged cross-file call graph that connects them.
#[derive(Debug, Clone, Default)]
pub struct InterProcInput {
    /// Facts per unit. Each unit's `CallFact.function`/`file` is used to bind
    /// its calls to `CallGraph` functions.
    pub units: Vec<ProgramFacts>,
    /// Merged, possibly cross-file call graph.
    pub call_graph: CallGraph,
}

impl InterProcInput {
    /// Build input from a list of `(file, facts)` pairs and an already-built
    /// cross-file call graph.
    pub fn new(units: Vec<ProgramFacts>, call_graph: CallGraph) -> Self {
        Self { units, call_graph }
    }
}

/// Run inter-procedural taint analysis.
///
/// Returns the union of every unit's findings *after* cross-file taint has
/// been propagated to fixpoint. Units that do not reach a sink remain silent,
/// so the output is directly comparable to a per-unit `analyze` run, just with
/// cross-file sources/sanitizers/sinks now connected.
pub fn analyze_interprocedural(input: &InterProcInput, rules: &RuleSet) -> Vec<TaintFinding> {
    // ---- Pass 1: seed the per-function return-taint summary ----------------
    let mut summary: HashMap<String, bool> = HashMap::new();

    // Set of source-rule ids so we can recognise source steps in a finding's
    // evidence chain by their `rule_id` (used to *extend* the summary with
    // findings surfaced after cross-file seeding).
    let source_rule_ids: HashSet<&str> = rules.sources.iter().map(|s| s.id.as_str()).collect();

    // Base summary: a function returns taint if, within its own body, it either
    // invokes a source rule or assigns a source into a field. This is derived
    // directly from the facts (not from findings), so it captures functions
    // like a `helper()` that *return* tainted data without themselves reaching
    // a sink — exactly the cross-file producers #843 was missing.
    for unit in &input.units {
        for call in &unit.calls {
            if rules.sources.iter().any(|s| s.symbol.matches(&call.symbol, None)) {
                summary.entry(call.function.clone()).or_insert(true);
            }
        }
        for fa in &unit.field_assigns {
            let is_source = fa.rhs_is_source
                || fa.rhs_call_id
                    .and_then(|id| unit.calls.iter().find(|c| c.id == id))
                    .map(|c| rules.sources.iter().any(|s| s.symbol.matches(&c.symbol, None)))
                    .unwrap_or(false);
            if is_source {
                summary.entry(fa.function.clone()).or_insert(true);
            }
        }
    }

    // Cache each unit's findings so Pass 2 can reuse and merge.
    let mut unit_findings: Vec<Vec<TaintFinding>> = Vec::with_capacity(input.units.len());
    for unit in &input.units {
        let findings = analyze(unit, rules).unwrap_or_default();
        unit_findings.push(findings);
    }

    // ---- Pass 2: worklist fixpoint over cross-file call edges --------------
    // First, propagate the return-taint summary across call edges (a caller of
    // a tainted callee also returns taint), then re-solve each unit with the
    // cross-file seeds injected.
    let mut guard = 0usize;
    const MAX_ITERS: usize = 32;
    let mut changed = true;
    while changed && guard < MAX_ITERS {
        guard += 1;
        changed = false;

        // Propagate return-taint across call edges once per outer iteration.
        for func in input.call_graph.functions() {
            if *summary.get(&func.name).unwrap_or(&false) {
                continue;
            }
            let tainted_callee = input
                .call_graph
                .calls_of(func.id)
                .iter()
                .any(|e| *summary.get(&e.callee_name).unwrap_or(&false));
            if tainted_callee {
                summary.insert(func.name.clone(), true);
                changed = true;
            }
        }

        let mut next_findings: Vec<Vec<TaintFinding>> = Vec::with_capacity(input.units.len());
        for unit in &input.units {
            let unit_file = unit
                .calls
                .first()
                .map(|c| c.file.clone())
                .or_else(|| unit.field_assigns.first().map(|f| f.file.clone()))
                .unwrap_or_default();

            // Build the return-taint seed for this unit: every call site whose
            // callee (by symbol, via the call graph) returns taint gets its
            // return value marked tainted, so intra-procedural flow can carry it
            // to a local sink.
            let mut ret_seed: HashMap<usize, TaintTag> = HashMap::new();
            seed_unit_from_summary(&input.call_graph, &unit_file, unit, &summary, &mut ret_seed);

            let findings = analyze_with_seed(unit, rules, &HashMap::new(), &ret_seed)
                .unwrap_or_default();

            // Extend the summary with any source step surfaced by the new
            // findings (a tainted return may now appear in a function that
            // previously looked clean).
            for f in &findings {
                for step in &f.chain {
                    if let Some(rid) = step.rule_id.as_deref() {
                        if source_rule_ids.contains(rid) {
                            if let Some(func) = function_at(&input.call_graph, &step.file, step.line) {
                                if !*summary.get(&func).unwrap_or(&false) {
                                    summary.insert(func, true);
                                    changed = true;
                                }
                            }
                        }
                    }
                }
            }
            next_findings.push(findings);
        }
        unit_findings = next_findings;
    }

    // ---- Merge all unit findings -------------------------------------------
    let mut all = Vec::new();
    for f in unit_findings {
        all.extend(f);
    }
    all
}

/// Seed a unit's call *returns* from the cross-file return-taint summary.
///
/// For every function `caller` defined in `unit_file`, for every outgoing call
/// edge whose resolved `callee_name` returns taint, the matching call site in
/// `unit` (same symbol) gets its return value marked tainted. This conservatively
/// models "calling a function that returns untrusted data", which then flows
/// through propagators / field assignments to a local sink — exactly the
/// cross-file gadget chain #843 was missing.
fn seed_unit_from_summary(
    cg: &CallGraph,
    unit_file: &str,
    unit: &ProgramFacts,
    summary: &HashMap<String, bool>,
    ret_seed: &mut HashMap<usize, TaintTag>,
) {
    for func in cg.functions() {
        if func.file != unit_file {
            continue;
        }
        for edge in cg.calls_of(func.id) {
            let callee_tainted = *summary.get(&edge.callee_name).unwrap_or(&false);
            if !callee_tainted {
                continue;
            }
            // Mark every call site in this unit matching the callee symbol.
            for call in &unit.calls {
                if call.symbol == edge.callee_name {
                    ret_seed.insert(call.id, TaintTag::all());
                }
            }
        }
    }
}

/// Resolve the enclosing function name for a call at `(file, line)` using the
/// call graph. Returns `None` if no function in `file` contains the line.
fn function_at(cg: &CallGraph, file: &str, line: usize) -> Option<String> {
    let mut best: Option<(&Function, usize)> = None;
    for func in cg.functions() {
        if func.file != file {
            continue;
        }
        if func.line <= line {
            match best {
                None => best = Some((func, func.line)),
                Some((_, best_line)) if func.line > best_line => best = Some((func, func.line)),
                _ => {}
            }
        }
    }
    best.map(|(f, _)| f.name.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::callgraph::CallGraph;
    use crate::rules::{RuleSet, SinkRule, SourceRule, SymbolSpec};

    fn symbol(s: &str) -> SymbolSpec {
        SymbolSpec {
            symbol: s.to_string(),
            arg: None,
            ret: false,
        }
    }

    fn test_rules() -> RuleSet {
        let mut r = RuleSet::default();
        r.sources.push(SourceRule {
            id: "src-taint".to_string(),
            language: "*".to_string(),
            symbol: symbol("source"),
            category: "test".to_string(),
            cwe: vec!["T".to_string()],
        });
        r.sinks.push(SinkRule {
            id: "sink-taint".to_string(),
            language: "*".to_string(),
            symbol: symbol("sink"),
            category: "test".to_string(),
            cwe: vec!["T".to_string()],
            severity: "high".to_string(),
        });
        r
    }

    /// Build a single-unit facts set where a source field is assigned and then
    /// read into a sink, exercising the field-sensitivity path.
    fn single_file_facts(file: &str) -> ProgramFacts {
        let mut u = ProgramFacts::new();
        // call 0: source() producing tainted data.
        u.calls.push(crate::taint::CallFact {
            id: 0,
            symbol: "source".to_string(),
            function: "entry".to_string(),
            file: file.to_string(),
            line: 2,
            args: vec![],
            arg_count: 0,
        });
        // field assignment: x.v = source() (rhs is call 0).
        u.field_assigns.push(crate::taint::FieldAssign {
            id: 100,
            base: "x".to_string(),
            field: "v".to_string(),
            rhs_call_id: Some(0),
            rhs_is_source: false,
            source_rule_id: None,
            function: "entry".to_string(),
            file: file.to_string(),
            line: 2,
        });
        // call 1: sink(x.v) — reads the tainted field.
        u.calls.push(crate::taint::CallFact {
            id: 1,
            symbol: "sink".to_string(),
            function: "entry".to_string(),
            file: file.to_string(),
            line: 3,
            args: vec![0],
            arg_count: 1,
        });
        u.field_refs.push(crate::taint::FieldRef {
            id: 201,
            base: "x".to_string(),
            field: "v".to_string(),
            call_id: 1,
            arg_pos: 0,
        });
        u
    }

    #[test]
    fn intra_file_source_reaches_sink() {
        let u = single_file_facts("A.rs");
        let cg = CallGraph::default();
        let input = InterProcInput::new(vec![u], cg);
        let rules = test_rules();
        let findings = analyze_interprocedural(&input, &rules);
        assert!(
            findings.iter().any(|f| f.sink_symbol == "sink"),
            "intra-file taint should reach sink; got {findings:?}"
        );
    }

    /// Cross-file fixture:
    ///   fileA `entry`: x.v = helper(); sink(x.v)   (helper defined in fileB)
    ///   fileB `helper`: y.v = source(); return y   (source tainted)
    ///
    /// Intra-procedural analysis of fileA alone finds nothing (helper is a
    /// black box). Inter-procedural analysis must connect `source` (fileB) to
    /// `sink` (fileA) across the file boundary — the #843 L2+ case.
    #[test]
    fn cross_file_source_reaches_sink() {
        // fileA: helper() return is stored, then sunk.
        let mut a = single_file_facts("A.rs");
        // Re-point the source-producing call to `helper` instead of `source`.
        a.calls[0].symbol = "helper".to_string();
        a.field_assigns[0].rhs_call_id = Some(0);

        let mut b = ProgramFacts::new();
        // fileB helper(): y.v = source(); (the return is the tainted field).
        b.calls.push(crate::taint::CallFact {
            id: 0,
            symbol: "source".to_string(),
            function: "helper".to_string(),
            file: "B.rs".to_string(),
            line: 2,
            args: vec![],
            arg_count: 0,
        });
        b.field_assigns.push(crate::taint::FieldAssign {
            id: 100,
            base: "y".to_string(),
            field: "v".to_string(),
            rhs_call_id: Some(0),
            rhs_is_source: false,
            source_rule_id: None,
            function: "helper".to_string(),
            file: "B.rs".to_string(),
            line: 2,
        });

        // Call graph: entry(A) -> helper(B); helper(B) has no outgoing calls.
        let mut cg = CallGraph::default();
        cg.add_function("entry", "A.rs", 1);
        cg.add_function("helper", "B.rs", 1);
        cg.add_call("entry", "helper");

        let input = InterProcInput::new(vec![a, b], cg);
        let rules = test_rules();
        let findings = analyze_interprocedural(&input, &rules);
        assert!(
            !findings.is_empty(),
            "expected a cross-file taint finding, got none"
        );
        assert!(
            findings.iter().any(|f| f.sink_symbol == "sink"),
            "cross-file flow source(B) -> sink(A) not detected; got {findings:?}"
        );
    }
}
