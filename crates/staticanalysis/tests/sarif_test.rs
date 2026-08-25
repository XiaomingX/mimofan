use mimofan_staticanalysis::sarif::{SarifLog, issue_dedup_key};

const SARIF: &str = r#"{
      "version": "2.1.0",
      "runs": [
        {
          "tool": { "driver": { "name": "semgrep" } },
          "results": [
            {
              "ruleId": "java.jndi-injection",
              "level": "error",
              "message": { "text": "JNDI lookup with tainted name" },
              "ruleIndex": 0,
              "properties": { "category": "jndi-injection", "cwe": ["CWE-74"] },
              "locations": [
                { "physicalLocation": {
                    "artifactLocation": { "uri": "DataSource.java" },
                    "region": { "startLine": 21 }
                }}
              ],
              "codeFlows": [ { "threads": [] } ]
            }
          ]
        }
      ]
    }"#;

#[test]
fn parses_and_normalizes_sarif() {
    let log = SarifLog::from_json(SARIF).unwrap();
    assert_eq!(log.schema_version, "2.1.0");
    assert_eq!(log.runs.len(), 1);
    assert_eq!(log.runs[0].tool_name, "semgrep");
    let issues = log.to_issues();
    assert_eq!(issues.len(), 1);
    let i = &issues[0];
    assert_eq!(i.severity, "error");
    assert_eq!(i.category, "jndi-injection");
    assert_eq!(i.cwe, vec!["CWE-74"]);
    assert_eq!(i.path.as_deref(), Some("DataSource.java"));
    assert_eq!(i.line, Some(21));
    assert!(!i.evidence.is_empty());
    assert_eq!(
        issue_dedup_key(i),
        "semgrep:DataSource.java:21:java.jndi-injection"
    );
}

#[test]
fn empty_runs_ok() {
    let log = SarifLog::from_json(r#"{"version":"2.1.0","runs":[]}"#).unwrap();
    assert!(log.to_issues().is_empty());
}
