use mimofan_staticanalysis::rules::{RuleSet, SinkRule, SourceRule, SymbolSpec};
use mimofan_staticanalysis::summary::{SummaryKind, SummaryTable};

#[test]
fn summary_classifies_rules() {
    let mut rules = RuleSet::default();
    rules.sources.push(SourceRule {
        id: "s".into(),
        language: "*".into(),
        symbol: SymbolSpec {
            symbol: "getParam".into(),
            arg: None,
            ret: true,
        },
        category: "x".into(),
        cwe: vec!["CWE-79".into()],
    });
    rules.sinks.push(SinkRule {
        id: "k".into(),
        language: "*".into(),
        symbol: SymbolSpec {
            symbol: "lookup".into(),
            arg: Some(0),
            ret: false,
        },
        category: "x".into(),
        cwe: vec!["CWE-74".into()],
        severity: "error".into(),
    });
    let table = SummaryTable::from_rules(&rules);
    assert_eq!(table.lookup("getParam").unwrap().kind, SummaryKind::Source);
    assert_eq!(table.lookup("lookup").unwrap().kind, SummaryKind::Sink);
    // Structural heuristic.
    assert_eq!(
        table.lookup("getJndiName").unwrap().kind,
        SummaryKind::Getter
    );
    assert_eq!(
        table.lookup("newBuilder").unwrap().kind,
        SummaryKind::Constructor
    );
}
