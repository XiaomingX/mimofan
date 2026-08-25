use mimofan_staticanalysis::knowledge::{KnowledgeBase, load_kb_dir};

const KB: &str = r#"
gadgets:
  - id: c3p0-jndi
    library: com.mchange:c3p0
    class: com.mchange.v2.c3p0.impl.JndiRefForwardingDataSource
    pivot: setJndiName
    enables: jndi-injection
    references: [CVE-2019-5427]
chains:
  - id: c3p0-log4shell
    name: C3P0 -> JNDI injection
    enables: jndi-injection
    requires: [c3p0-jndi, jndi-lookup]
    severity: error
    references: [CVE-2021-44228]
patterns:
  - id: pat-jndi-lookup
    language: java
    symbol: InitialContext.lookup
    arg: 0
    category: jndi-injection
    cwe: [CWE-74]
"#;

#[test]
fn loads_real_kb_from_disk() {
    // Prove the shipped KB data is real, not a stub: the kb dir must parse
    // into actual gadgets/chains/patterns.
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src/rules/kb");
    let kb = load_kb_dir(dir).expect("load kb dir");
    assert!(
        !kb.is_empty(),
        "expected gadgets/chains/patterns from on-disk KB"
    );
}

#[test]
fn loads_and_matches_chain() {
    let mut kb = KnowledgeBase::default();
    kb.extend_from_yaml("kb.yaml", KB).unwrap();
    assert!(kb.gadgets.contains_key("c3p0-jndi"));
    assert_eq!(kb.chains.len(), 1);
    assert_eq!(kb.patterns.len(), 1);

    // Chain requires c3p0-jndi AND jndi-lookup; only one present -> not satisfied.
    let partial = kb.satisfied_chains(&["c3p0-jndi".to_string()]);
    assert!(partial.is_empty());

    let full = kb.satisfied_chains(&["c3p0-jndi".to_string(), "jndi-lookup".to_string()]);
    assert_eq!(full.len(), 1);
    assert_eq!(full[0].id, "c3p0-log4shell");
}
