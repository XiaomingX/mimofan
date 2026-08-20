//! Access-Control static analysis.
//!
//! Mirrors the AVDH "Access Control agent": for each entry point, decide
//! whether an authorization gate is *missing* or the wrong identity is used.
//! mimofan has no engine for this today; this module adds a self-contained,
//! call-graph-reachability based detector.
//!
//! Core logic: for each entry-point function, determine whether any
//! authorization-gate method is reachable from it (via `CallGraph` same-file
//! reachability). Reachable => protected; unreachable => exposed.
//!
//! All detection here is **suffix-name based**: a function is an entry point
//! when its simple name ends with a configured entry suffix, and a function is
//! a gate when its name ends with a configured gate suffix. This is a
//! heuristic — see the module docs for known limitations.

use crate::callgraph::CallGraph;
use crate::sarif::SecurityIssue;
use crate::{AstError, Language};
use std::sync::OnceLock;

/// A single authorization-gate pattern. Suffix-matched against callable names.
#[derive(Clone)]
pub struct GateSpec {
    pub symbol: String,
    pub severity: String,
    pub category: String,
    pub cwe: Vec<String>,
}

/// A single entry-point pattern. Suffix-matched against function names.
#[derive(Clone)]
pub struct EntrySpec {
    pub symbol: String,
    pub severity: String,
    pub category: String,
    pub cwe: Vec<String>,
}

/// Default gate method suffixes (suffix-match; add reasonable coverage).
///
/// `Vec`/`String` fields are not const-constructible, so the tables are built
/// once behind a `OnceLock` and exposed as `&'static [GateSpec]`.
#[allow(non_snake_case)]
pub fn DEFAULT_GATES() -> &'static [GateSpec] {
    static GATES: OnceLock<Vec<GateSpec>> = OnceLock::new();
    GATES.get_or_init(|| {
        vec![
            GateSpec { symbol: "require_role".into(), severity: "warning".into(), category: "missing-authorization".into(), cwe: vec!["CWE-862".into()] },
            GateSpec { symbol: "check_permission".into(), severity: "warning".into(), category: "missing-authorization".into(), cwe: vec!["CWE-862".into()] },
            GateSpec { symbol: "require_auth".into(), severity: "warning".into(), category: "missing-authentication".into(), cwe: vec!["CWE-306".into()] },
            GateSpec { symbol: "require_login".into(), severity: "warning".into(), category: "missing-authentication".into(), cwe: vec!["CWE-306".into()] },
            GateSpec { symbol: "require_permission".into(), severity: "warning".into(), category: "missing-authorization".into(), cwe: vec!["CWE-862".into()] },
            GateSpec { symbol: "authorize".into(), severity: "warning".into(), category: "missing-authorization".into(), cwe: vec!["CWE-862".into()] },
            GateSpec { symbol: "ensure_authenticated".into(), severity: "warning".into(), category: "missing-authentication".into(), cwe: vec!["CWE-306".into()] },
            GateSpec { symbol: "is_admin".into(), severity: "warning".into(), category: "missing-authorization".into(), cwe: vec!["CWE-862".into()] },
            GateSpec { symbol: "has_role".into(), severity: "warning".into(), category: "missing-authorization".into(), cwe: vec!["CWE-862".into()] },
        ]
    })
}

/// Default entry-point name suffixes.
#[allow(non_snake_case)]
pub fn DEFAULT_ENTRIES() -> &'static [EntrySpec] {
    static ENTRIES: OnceLock<Vec<EntrySpec>> = OnceLock::new();
    ENTRIES.get_or_init(|| {
        vec![
            EntrySpec { symbol: "handler".into(), severity: "high".into(), category: "unauthenticated-entry-point".into(), cwe: vec!["CWE-306".into()] },
            EntrySpec { symbol: "endpoint".into(), severity: "high".into(), category: "unauthenticated-entry-point".into(), cwe: vec!["CWE-306".into()] },
            EntrySpec { symbol: "route".into(), severity: "high".into(), category: "unauthenticated-entry-point".into(), cwe: vec!["CWE-306".into()] },
            EntrySpec { symbol: "on_request".into(), severity: "high".into(), category: "unauthenticated-entry-point".into(), cwe: vec!["CWE-306".into()] },
        ]
    })
}

/// Analyze a single source unit: report entry points with no reachable
/// authorization gate.
pub fn analyze_file(
    file: &str,
    source: &str,
    lang: Language,
    entries: &[EntrySpec],
    gates: &[GateSpec],
) -> Result<Vec<SecurityIssue>, AstError> {
    let graph = CallGraph::build(file, source, lang)?;
    Ok(analyze_graph(&graph, file, entries, gates))
}

/// Analyze an already-built call graph (also used for Java cross-file).
pub fn analyze_graph(
    graph: &CallGraph,
    file: &str,
    entries: &[EntrySpec],
    gates: &[GateSpec],
) -> Vec<SecurityIssue> {
    let mut issues = Vec::new();
    for func in graph.functions() {
        // Only consider functions whose name matches an entry pattern.
        let Some(entry_spec) = entries.iter().find(|e| func.name.ends_with(&e.symbol)) else {
            continue;
        };
        // Skip if the entry function itself is a gate (defensive).
        if gates.iter().any(|g| func.name.ends_with(&g.symbol)) {
            continue;
        }
        let reachable = graph.reachable_from(func.id);
        // Is any reachable function (incl. the entry) a gate?
        let has_gate = reachable.iter().any(|&id| {
            graph
                .function_name(id)
                .map(|n| gates.iter().any(|g| n.ends_with(&g.symbol)))
                .unwrap_or(false)
        });
        if !has_gate {
            issues.push(SecurityIssue {
                tool: "access-control".into(),
                rule_id: "access-control.missing-gate".into(),
                severity: entry_spec.severity.clone(),
                category: entry_spec.category.clone(),
                title: format!(
                    "Entry point '{}' has no reachable authorization gate",
                    func.name
                ),
                description: format!(
                    "{} at {}:{} does not reach any of the authorization gates ({}) \
                     before doing work. Confirm privileged functionality is not exposed \
                     to unauthenticated/unauthorized users.",
                    func.name, file, func.line,
                    gates.iter().map(|g| g.symbol.as_str()).collect::<Vec<_>>().join(", ")
                ),
                cwe: entry_spec.cwe.clone(),
                path: Some(file.to_string()),
                line: Some(func.line as u32),
                evidence: vec![format!("entry={} line={} -> no gate reachable", func.name, func.line)],
                automated: true,
            });
        }
    }
    issues
}

/// Java cross-file: analyze all .java files under `dir` with a merged graph.
pub fn analyze_dir(
    dir: &str,
    entries: &[EntrySpec],
    gates: &[GateSpec],
) -> Vec<SecurityIssue> {
    let graph = CallGraph::build_from_dir(std::path::Path::new(dir));
    analyze_graph(&graph, dir, entries, gates)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn analyze_rust(source: &str) -> Vec<SecurityIssue> {
        analyze_file(
            "test.rs",
            source,
            Language::Rust,
            DEFAULT_ENTRIES(),
            DEFAULT_GATES(),
        )
        .expect("analyze_file should succeed on valid Rust")
    }

    #[test]
    fn exposed_handler_flagged() {
        let source = r#"
fn db_insert(user: &str) {
    // persists user
}

fn create_user_handler(req: &str) -> String {
    db_insert(req);
    "ok".to_string()
}
"#;
        let issues = analyze_rust(source);
        assert_eq!(issues.len(), 1, "expected exactly one access-control issue");
        assert_eq!(issues[0].tool, "access-control");
        assert_eq!(issues[0].rule_id, "access-control.missing-gate");
        assert_eq!(issues[0].severity, "high");
        assert!(issues[0].description.contains("create_user_handler"));
    }

    #[test]
    fn protected_handler_ok() {
        let source = r#"
fn require_role(role: &str) -> bool {
    role == "admin"
}

fn create_user_handler(req: &str) -> String {
    if !require_role("admin") {
        return "forbidden".to_string();
    }
    "ok".to_string()
}
"#;
        let issues = analyze_rust(source);
        assert_eq!(issues.len(), 0, "protected handler should not be flagged");
    }

    #[test]
    fn default_gates_nonempty() {
        assert!(!DEFAULT_GATES().is_empty());
        assert!(!DEFAULT_ENTRIES().is_empty());
        // Each default gate should carry a CWE id.
        for g in DEFAULT_GATES() {
            assert!(!g.cwe.is_empty(), "gate '{}' should have a CWE", g.symbol);
        }
    }
}
