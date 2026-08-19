use mimofan_staticanalysis::recon::{run, ReconBudget, ReconCapability};
use mimofan_staticanalysis::sarif::SecurityIssue;

struct FakeCap {
    name: &'static str,
    issues: Vec<SecurityIssue>,
}

impl ReconCapability for FakeCap {
    fn name(&self) -> &'static str {
        self.name
    }
    fn run(&self, _b: &ReconBudget) -> Vec<SecurityIssue> {
        self.issues.clone()
    }
}

fn issue(tool: &str, rule: &str) -> SecurityIssue {
    SecurityIssue {
        tool: tool.into(),
        rule_id: rule.into(),
        severity: "error".into(),
        category: "x".into(),
        title: rule.into(),
        description: "".into(),
        cwe: vec![],
        path: Some("a.java".into()),
        line: Some(1),
        evidence: vec![],
        automated: true,
    }
}

#[test]
fn runs_caps_in_parallel_and_dedupes() {
    let caps: Vec<Box<dyn ReconCapability>> = vec![
        Box::new(FakeCap {
            name: "taint",
            issues: vec![issue("taint", "R1"), issue("taint", "R2")],
        }),
        Box::new(FakeCap {
            name: "sca",
            issues: vec![issue("sca", "R1")], // dup of taint R1 on same path/line
        }),
    ];
    let budget = ReconBudget::default();
    let merged = run(&budget, caps).unwrap();
    // Add a true duplicate (same tool, rule, path, line) to exercise dedup.
    let caps2: Vec<Box<dyn ReconCapability>> = vec![
        Box::new(FakeCap {
            name: "taint",
            issues: vec![issue("taint", "R1"), issue("taint", "R2")],
        }),
        Box::new(FakeCap {
            name: "sca",
            issues: vec![issue("sca", "R1")],
        }),
        Box::new(FakeCap {
            name: "taint",
            issues: vec![issue("taint", "R1")],
        }),
    ];
    let merged2 = run(&budget, caps2).unwrap();
    assert_eq!(
        merged2.len(),
        3,
        "one duplicate should be removed: {merged2:?}"
    );
    assert_eq!(merged.len(), 3);
}
