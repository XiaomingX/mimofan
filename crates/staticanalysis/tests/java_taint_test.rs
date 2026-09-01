use mimofan_staticanalysis::java_taint::{analyze_file, java_rules};

#[test]
fn sqli_tainted_concat_to_executequery_is_flagged() {
    let src = std::fs::read_to_string("/tmp/sqli_vuln.java").unwrap();
    let issues = analyze_file("V.java", &src).unwrap();
    assert!(
        issues.iter().any(|i| i.cwe.contains(&"CWE-89".to_string())),
        "expected CWE-89 finding, got: {:?}",
        issues.iter().map(|i| &i.rule_id).collect::<Vec<_>>()
    );
}

#[test]
fn prepared_statement_safe_is_not_flagged() {
    let src = std::fs::read_to_string("/tmp/sqli_safe.java").unwrap();
    let issues = analyze_file("S.java", &src).unwrap();
    assert!(
        issues.is_empty(),
        "prepared statement must be clean, got: {issues:?}"
    );
}

#[test]
fn command_injection_runtime_exec_flagged() {
    let src = std::fs::read_to_string("/tmp/cmdi_vuln.java").unwrap();
    let issues = analyze_file("C.java", &src).unwrap();
    assert!(
        issues.iter().any(|i| i.cwe.contains(&"CWE-78".to_string())),
        "expected CWE-78 finding, got: {:?}",
        issues.iter().map(|i| &i.rule_id).collect::<Vec<_>>()
    );
}

#[test]
fn rules_load_with_expected_counts() {
    let rs = java_rules();
    assert!(rs.sources.len() >= 10);
    assert!(rs.sinks.len() >= 15);
}
