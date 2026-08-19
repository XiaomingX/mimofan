use mimofan_staticanalysis::rules::{
    PropagatorRule, RuleSet, SanitizerRule, SinkRule, SourceRule, SymbolSpec,
};
use mimofan_staticanalysis::taint::{analyze, CallFact, FieldAssign, FieldRef, ProgramFacts};

fn c3p0_rules() -> RuleSet {
    let yaml = r#"
sources:
  - id: java-jndi-name-source
    language: java
    symbol: setJndiName
    category: jndi-injection
    cwe: [CWE-74, CWE-502]
sinks:
  - id: java-jndi-lookup
    language: java
    symbol: InitialContext.lookup
    category: jndi-injection
    cwe: [CWE-74]
    severity: error
propagators:
  - id: java-string-concat
    language: java
    symbol: concat
    from_arg: 0
    to_receiver: false
sanitizers:
  - id: java-autotype-deny
    language: java
    symbol: isAutoTypeDenyClass
    neutralizes: []
"#;
    let mut set = RuleSet::default();
    set.extend_from_yaml("c3p0.yaml", yaml).unwrap();
    set
}

#[test]
fn c3p0_gadget_chain_detected() {
    // Models the C3P0 gadget chain at the data-flow level:
    //   dataSource.setJndiName(<untrusted>);          // source
    //   ... dataSource.jndiName stored ...
    //   InitialContext.lookup(dataSource.jndiName);   // sink
    // Field sensitivity (>=1 level) is exercised via the `jndiName` field:
    // the source assignment taints `dataSource.jndiName`, and the lookup
    // reads that field into its argument 0.
    let mut facts = ProgramFacts::new();

    // call 0: setJndiName(untrusted) — rule `java-jndi-name-source`.
    facts.calls.push(CallFact {
        id: 0,
        symbol: "setJndiName".into(),
        function: "configure".into(),
        file: "DataSource.java".into(),
        line: 10,
        args: vec![0],
        arg_count: 1,
    });
    // Field assignment: dataSource.jndiName = <untrusted> (from the source).
    facts.field_assigns.push(FieldAssign {
        id: 100,
        base: "dataSource".into(),
        field: "jndiName".into(),
        rhs_call_id: None,
        rhs_is_source: true,
        source_rule_id: Some("java-jndi-name-source".into()),
        function: "configure".into(),
        file: "DataSource.java".into(),
        line: 10,
    });
    // call 2: InitialContext.lookup(dataSource.jndiName) — sink.
    facts.calls.push(CallFact {
        id: 2,
        symbol: "InitialContext.lookup".into(),
        function: "lookup".into(),
        file: "DataSource.java".into(),
        line: 21,
        args: vec![0],
        arg_count: 1,
    });
    // Field read: lookup arg 0 = dataSource.jndiName (field sensitivity).
    facts.field_refs.push(FieldRef {
        id: 201,
        base: "dataSource".into(),
        field: "jndiName".into(),
        call_id: 2,
        arg_pos: 0,
    });

    let rules = c3p0_rules();
    let findings = analyze(&facts, &rules).unwrap();
    // Expect at least one finding reaching InitialContext.lookup.
    let hit = findings
        .iter()
        .find(|f| f.sink_symbol == "InitialContext.lookup");
    assert!(
        hit.is_some(),
        "expected C3P0 chain to reach JNDI lookup; got {findings:?}"
    );
    let hit = hit.unwrap();
    assert_eq!(hit.sink_rule_id, "java-jndi-lookup");
    // Chain should mention the source and the sink.
    let chain_text = format!("{:?}", hit.chain);
    assert!(
        chain_text.contains("java-jndi-name-readback") || chain_text.contains("source"),
        "chain should reference a source: {:#?}",
        hit.chain
    );
}

#[test]
fn cross_function_field_propagation_detected() {
    // Genuine cross-function evidence chain: a tainted field assigned in
    // function `configure` is read into a sink in a *different* function
    // `handle`. Exercises the field-driven evidence chain across function
    // boundaries (not just within one function).
    let mut facts = ProgramFacts::new();
    facts.field_assigns.push(FieldAssign {
        id: 100,
        base: "ctx".into(),
        field: "name".into(),
        rhs_call_id: None,
        rhs_is_source: true,
        source_rule_id: Some("java-jndi-name-source".into()),
        function: "configure".into(),
        file: "App.java".into(),
        line: 10,
    });
    facts.calls.push(CallFact {
        id: 2,
        symbol: "InitialContext.lookup".into(),
        function: "handle".into(),
        file: "App.java".into(),
        line: 30,
        args: vec![0],
        arg_count: 1,
    });
    facts.field_refs.push(FieldRef {
        id: 201,
        base: "ctx".into(),
        field: "name".into(),
        call_id: 2,
        arg_pos: 0,
    });

    let rules = c3p0_rules();
    let findings = analyze(&facts, &rules).unwrap();
    let hit = findings
        .iter()
        .find(|f| f.sink_symbol == "InitialContext.lookup");
    assert!(
        hit.is_some(),
        "cross-function field taint must reach sink; got {findings:?}"
    );
    // The chain must mention the source assignment (proving the full
    // source -> field -> sink evidence chain was reconstructed).
    let chain_text = format!("{:?}", hit.unwrap().chain);
    assert!(
        chain_text.contains("source") && chain_text.contains("field"),
        "chain should reconstruct source + field hop: {:#?}",
        hit.unwrap().chain
    );
}

#[test]
fn strong_sanitizer_prunes_finding() {
    let mut rules = RuleSet::default();
    rules.sources.push(SourceRule {
        id: "src".into(),
        language: "*".into(),
        symbol: SymbolSpec {
            symbol: "evil".into(),
            arg: None,
            ret: true,
        },
        category: "x".into(),
        cwe: vec!["CWE-79".into()],
    });
    rules.sinks.push(SinkRule {
        id: "snk".into(),
        language: "*".into(),
        symbol: SymbolSpec {
            symbol: "sink".into(),
            arg: Some(0),
            ret: false,
        },
        category: "xss".into(),
        cwe: vec!["CWE-79".into()],
        severity: "error".into(),
    });
    rules.sanitizers.push(SanitizerRule {
        id: "strong".into(),
        language: "*".into(),
        symbol: SymbolSpec {
            symbol: "clean".into(),
            arg: Some(0),
            ret: true,
        },
        neutralizes: vec![], // strong
    });

    let mut facts = ProgramFacts::new();
    facts.calls.push(CallFact {
        id: 0,
        symbol: "evil".into(),
        function: "f".into(),
        file: "x.java".into(),
        line: 1,
        args: vec![],
        arg_count: 0,
    });
    facts.calls.push(CallFact {
        id: 1,
        symbol: "clean".into(),
        function: "f".into(),
        file: "x.java".into(),
        line: 2,
        args: vec![0],
        arg_count: 1,
    });
    // sink takes clean's (now sanitized) return.
    facts.calls.push(CallFact {
        id: 2,
        symbol: "sink".into(),
        function: "f".into(),
        file: "x.java".into(),
        line: 3,
        args: vec![0],
        arg_count: 1,
    });
    // Wire: clean(0) arg0 = evil() return (tainted) -> clean return clean.
    // sink(0) = clean return. Mark arg_taint accordingly is done by solver
    // via propagator/return flow; but our solver tracks ret_taint and
    // propagators only. To exercise the sanitizer path we add a propagator
    // mapping clean's arg0->ret.
    rules.propagators.push(PropagatorRule {
        id: "clean-prop".into(),
        language: "*".into(),
        symbol: SymbolSpec {
            symbol: "clean".into(),
            arg: None,
            ret: false,
        },
        from_arg: Some(0),
        to_receiver: false,
    });

    let findings = analyze(&facts, &rules).unwrap();
    // Strong sanitizer clears all taint, so the sink must NOT fire.
    assert!(
        findings.iter().all(|f| f.sink_symbol != "sink"),
        "strong sanitizer should prune the finding, got {findings:?}"
    );
}
