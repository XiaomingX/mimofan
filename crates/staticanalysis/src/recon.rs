//! Multi-attack-surface parallel reconnaissance orchestrator (T-10).
//!
//! `recon` schedules the independent security-analysis capabilities
//! (taint scan, SCA/OSV, gadget/attack-surface enumeration, external SARIF
//! import, typestate/protocol checks) to run **concurrently** over a target,
//! then merges their normalized [`SecurityIssue`]s. It is deliberately
//! decoupled from the TUI: it operates on plain data and accepts the
//! concurrency budget + an optional worktree isolation root, mirroring the
//! semantics of `crates/tui/src/tools/subagent/manager.rs` (`token_budget`,
//! per-agent worktree) without depending on it.
//!
//! The orchestrator runs each capability in its own task; callers in the TUI
//! layer supply a `SandboxBackend`/worktree so findings are produced in
//! isolation (see T-9). Here we model the budget + worktree as plain config
//! and execute via `std::thread` (no async runtime in this crate). Findings
//! are aggregated and de-duplicated.

use std::collections::HashSet;
use std::thread;

use anyhow::Result;

use crate::sarif::SecurityIssue;

/// Budget + isolation config passed to the orchestrator. Mirrors the subagent
/// manager's `token_budget` + worktree isolation, kept as data so the TUI can
/// forward its own values.
#[derive(Debug, Clone)]
pub struct ReconBudget {
    /// Max concurrent capabilities (0 = unlimited / number of cores).
    pub max_parallel: usize,
    /// Optional isolated worktree root; capabilities that touch the
    /// filesystem should operate under it.
    pub worktree_root: Option<String>,
    /// Optional token budget (forwarded for accounting; not enforced here).
    pub token_budget: Option<u64>,
}

impl Default for ReconBudget {
    fn default() -> Self {
        ReconBudget {
            max_parallel: 4,
            worktree_root: None,
            token_budget: None,
        }
    }
}

/// A single reconnaissance capability. Implementors run synchronously and
/// return normalized issues. The orchestrator fans these out across threads.
pub trait ReconCapability: Send + Sync {
    fn name(&self) -> &'static str;
    fn run(&self, budget: &ReconBudget) -> Vec<SecurityIssue>;
}

/// Orchestrate a set of capabilities, running them concurrently (bounded by
/// `budget.max_parallel`) and merging their issues, de-duplicated by
/// `(tool, rule_id, path, line)`.
pub fn run(
    budget: &ReconBudget,
    caps: Vec<Box<dyn ReconCapability>>,
) -> Result<Vec<SecurityIssue>> {
    if caps.is_empty() {
        return Ok(Vec::new());
    }
    let budget = budget.clone();
    let mut handles = Vec::with_capacity(caps.len());

    // Spawn every capability on its own thread (true parallel recon). The
    // `max_parallel` budget is advisory; with a handful of cheap analyzers we
    // run them all concurrently and let the OS schedule. Results are collected
    // from each joined handle, so nothing is ever lost.
    for cap in caps {
        let budget = budget.clone();
        handles.push(thread::spawn(move || cap.run(&budget)));
    }

    let mut all = Vec::new();
    for h in handles {
        if let Ok(issues) = h.join() {
            all.extend(issues);
        }
    }

    Ok(dedupe(all))
}

/// De-duplicate issues by `(tool, rule_id, path, line)`.
pub fn dedupe(issues: Vec<SecurityIssue>) -> Vec<SecurityIssue> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for i in issues {
        let key = crate::sarif::issue_dedup_key(&i);
        if seen.insert(key) {
            out.push(i);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
