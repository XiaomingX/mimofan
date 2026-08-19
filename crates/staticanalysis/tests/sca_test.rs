use mimofan_staticanalysis::sca::{
    prune_unreachable, Advisory, InMemoryOsv, parse_cargo_lock, parse_npm_lock, scan,
};

#[test]
fn parses_cargo_lock() {
    let lock = r#"
[[package]]
name = "serde"
version = "1.0.190"

[[package]]
name = "bad-crate"
version = "0.1.0"
"#;
    let deps = parse_cargo_lock(lock).unwrap();
    assert_eq!(deps.len(), 2);
    assert_eq!(deps[0].name, "serde");
    assert_eq!(deps[0].ecosystem, "crates.io");
    assert!(deps[1].reachable);
}

#[test]
fn parses_npm_lock() {
    let lock = r#"{
          "packages": {
            "node_modules/lodash": { "version": "4.17.20" },
            "node_modules/express": { "version": "4.18.2" }
          }
        }"#;
    let deps = parse_npm_lock(lock).unwrap();
    assert_eq!(deps.len(), 2);
    assert!(
        deps.iter()
            .any(|d| d.name == "lodash" && d.version == "4.17.20")
    );
}

#[test]
fn osv_match_and_prune() {
    let mut mem = InMemoryOsv::default();
    mem.advisories.insert(
        ("crates.io".to_string(), "bad-crate".to_string()),
        vec![Advisory {
            id: "OSV-1".to_string(),
            summary: "RCE in bad-crate".into(),
            severity: "critical".into(),
            aliases: vec!["CVE-2024-1".into()],
            vulnerable_range: "<0.2.0".into(),
        }],
    );
    let lock = r#"
[[package]]
name = "bad-crate"
version = "0.1.0"
"#;
    let findings = scan("Cargo.lock", lock, &mem).unwrap();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].advisory.id, "OSV-1");

    // Prune: if bad-crate is not reachable, drop it.
    let pruned = prune_unreachable(findings.clone(), &["other-crate".to_string()]);
    assert!(pruned.is_empty());
    let kept = prune_unreachable(findings, &["bad-crate".to_string()]);
    assert_eq!(kept.len(), 1);
}
