use mimofan_staticanalysis::callgraph::CallGraph;
use mimofan_staticanalysis::interproc::{InterProcInput, analyze_interprocedural};
use mimofan_staticanalysis::rules::{RuleSet, SinkRule, SourceRule, SymbolSpec};
use mimofan_staticanalysis::taint::{CallFact, FieldAssign, FieldRef, ProgramFacts};

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
    u.calls.push(CallFact {
        id: 0,
        symbol: "source".to_string(),
        function: "entry".to_string(),
        file: file.to_string(),
        line: 2,
        args: vec![],
        arg_count: 0,
    });
    // field assignment: x.v = source() (rhs is call 0).
    u.field_assigns.push(FieldAssign {
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
    u.calls.push(CallFact {
        id: 1,
        symbol: "sink".to_string(),
        function: "entry".to_string(),
        file: file.to_string(),
        line: 3,
        args: vec![0],
        arg_count: 1,
    });
    u.field_refs.push(FieldRef {
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
    b.calls.push(CallFact {
        id: 0,
        symbol: "source".to_string(),
        function: "helper".to_string(),
        file: "B.rs".to_string(),
        line: 2,
        args: vec![],
        arg_count: 0,
    });
    b.field_assigns.push(FieldAssign {
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
