use mimofan_staticanalysis::attack_surface::{AttackSurfaceKind, enumerate_surface};
use mimofan_staticanalysis::knowledge::KnowledgeBase;
use mimofan_staticanalysis::rules::{RuleSet, SymbolSpec};
use mimofan_staticanalysis::sca::{Advisory, Dependency, InMemoryOsv};

#[test]
fn enumerates_c3p0_chain_and_vuln_dep() {
    let mut kb = KnowledgeBase::default();
    kb.extend_from_yaml(
        "kb.yaml",
        r#"
gadgets:
  - id: c3p0-jndi
    library: com.mchange:c3p0
    class: JndiRefForwardingDataSource
    pivot: setJndiName
    enables: jndi-injection
    references: [CVE-2019-5427]
  - id: jndi-lookup
    library: javax.naming
    class: InitialContext
    pivot: lookup
    enables: jndi-injection
    references: []
chains:
  - id: c3p0-chain
    name: C3P0 -> JNDI
    enables: jndi-injection
    requires: [c3p0-jndi, jndi-lookup]
    severity: error
    references: [CVE-2021-44228]
"#,
    )
    .unwrap();

    let deps = vec![
        Dependency {
            name: "com.mchange:c3p0".into(),
            version: "0.9.5.5".into(),
            ecosystem: "maven".into(),
            reachable: true,
        },
        Dependency {
            name: "javax.naming".into(),
            version: "1.0".into(),
            ecosystem: "maven".into(),
            reachable: true,
        },
    ];

    let mut osv = InMemoryOsv::default();
    osv.advisories.insert(
        ("maven".to_string(), "com.mchange:c3p0".to_string()),
        vec![Advisory {
            id: "OSV-C3P0".to_string(),
            summary: "JNDI injection in c3p0".into(),
            severity: "critical".into(),
            aliases: vec!["CVE-2019-5427".into()],
            vulnerable_range: "<0.9.5.6".into(),
        }],
    );

    let advisories = vec![(
        deps[0].clone(),
        osv.advisories
            .get(&("maven".to_string(), "com.mchange:c3p0".to_string()))
            .unwrap()[0]
            .clone(),
    )];

    let entries = enumerate_surface(&kb, &deps, &advisories);
    let has_chain = entries.iter().any(|e| {
        e.kind == AttackSurfaceKind::GadgetChain && e.ref_id.as_deref() == Some("c3p0-chain")
    });
    let has_vuln = entries
        .iter()
        .any(|e| e.kind == AttackSurfaceKind::VulnerableDependency);
    assert!(
        has_chain,
        "C3P0 gadget chain should be enumerated: {entries:?}"
    );
    assert!(has_vuln, "vulnerable dependency should be enumerated");
}

#[test]
fn implicit_autotype_flagged() {
    let kb = KnowledgeBase::default();
    let deps = vec![Dependency {
        name: "com.alibaba:fastjson".into(),
        version: "1.2.60".into(),
        ecosystem: "maven".into(),
        reachable: true,
    }];
    let entries = enumerate_surface(&kb, &deps, &[]);
    assert!(
        entries
            .iter()
            .any(|e| e.kind == AttackSurfaceKind::ImplicitAutoType)
    );
}

// Touch RuleSet/SymbolSpec imports so the test module compiles even if the
// KB test above is the only consumer in this file.
#[test]
fn _unused_import_guard() {
    let _ = RuleSet::default();
    let _ = SymbolSpec {
        symbol: "x".into(),
        arg: None,
        ret: false,
    };
}
