//! SARIF parsing & normalization (T-6).
//!
//! External analyzers (semgrep, CodeQL, SpotBugs, etc.) emit
//! [SARIF 2.1.0](https://docs.oasis-open.org/sarif/sarif/v2.1.0/). This module
//! parses a SARIF log into a normalized form and converts each result into the
//! crate's internal [`SecurityIssue`] shape, so downstream reporting (the TUI
//! reviewer's `security_issues`, see `crates/tui/src/tools/review.rs`) can
//! present SAST output uniformly regardless of the originating tool.
//!
//! We depend only on `serde_json` (the crate's available dependency) and parse
//! the JSON directly rather than pulling in a SARIF schema crate.

use anyhow::{Context, Result};

/// Normalized security issue, the common currency between all analyzers and the
/// TUI reviewer. Mirrors the fields the reviewer's `security_issues` carries
/// (category, cwe, taint provenance) so a SARIF result slots straight in.
#[derive(Debug, Clone, PartialEq)]
pub struct SecurityIssue {
    pub tool: String,
    pub rule_id: String,
    pub severity: String,
    pub category: String,
    pub title: String,
    pub description: String,
    pub cwe: Vec<String>,
    pub path: Option<String>,
    pub line: Option<u32>,
    /// Optional structured evidence chain (e.g. taint trace) attached by
    /// upstream tools that emit `properties.taint` / `codeFlows`.
    pub evidence: Vec<String>,
    /// True if the issue came from automated analysis (vs. human review).
    pub automated: bool,
}

/// A parsed SARIF log.
#[derive(Debug, Clone)]
pub struct SarifLog {
    pub schema_version: String,
    pub runs: Vec<SarifRun>,
}

#[derive(Debug, Clone)]
pub struct SarifRun {
    pub tool_name: String,
    pub results: Vec<SarifResult>,
}

#[derive(Debug, Clone)]
pub struct SarifResult {
    pub rule_id: String,
    pub level: String,
    pub message: String,
    pub rule_index: Option<usize>,
    pub cwe: Vec<String>,
    pub category: Option<String>,
    pub path: Option<String>,
    pub line: Option<u32>,
    pub code_flows: Vec<String>,
}

impl SarifLog {
    /// Parse a SARIF JSON document.
    pub fn from_json(text: &str) -> Result<SarifLog> {
        let v: serde_json::Value =
            serde_json::from_str(text).context("SARIF must be valid JSON")?;
        let schema = v
            .get("version")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();

        let runs = v
            .get("runs")
            .and_then(|x| x.as_array())
            .context("SARIF missing `runs` array")?;

        let mut out_runs = Vec::new();
        for run in runs {
            let tool_name = run
                .get("tool")
                .and_then(|t| t.get("driver"))
                .and_then(|d| d.get("name"))
                .and_then(|n| n.as_str())
                .unwrap_or("unknown")
                .to_string();

            let mut results = Vec::new();
            if let Some(arr) = run.get("results").and_then(|x| x.as_array()) {
                for r in arr {
                    results.push(parse_result(r));
                }
            }
            out_runs.push(SarifRun { tool_name, results });
        }
        Ok(SarifLog {
            schema_version: schema,
            runs: out_runs,
        })
    }

    /// Normalize every SARIF result into a [`SecurityIssue`].
    pub fn to_issues(&self) -> Vec<SecurityIssue> {
        let mut issues = Vec::new();
        for run in &self.runs {
            for r in &run.results {
                issues.push(SecurityIssue {
                    tool: run.tool_name.clone(),
                    rule_id: r.rule_id.clone(),
                    severity: sarif_level_to_severity(&r.level),
                    category: r.category.clone().unwrap_or_else(|| r.rule_id.clone()),
                    title: r.rule_id.clone(),
                    description: r.message.clone(),
                    cwe: r.cwe.clone(),
                    path: r.path.clone(),
                    line: r.line,
                    evidence: r.code_flows.clone(),
                    automated: true,
                });
            }
        }
        issues
    }
}

fn parse_result(r: &serde_json::Value) -> SarifResult {
    let rule_id = r
        .get("ruleId")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let level = r
        .get("level")
        .and_then(|x| x.as_str())
        .unwrap_or("warning")
        .to_string();
    let message = r
        .get("message")
        .and_then(|m| m.get("text"))
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();

    let rule_index = r
        .get("ruleIndex")
        .and_then(|x| x.as_u64())
        .map(|u| u as usize);

    // CWE tags may live in `properties` or in the rule's `helpUri`/shortDescription.
    let mut cwe = Vec::new();
    if let Some(props) = r.get("properties")
        && let Some(c) = props.get("cwe").and_then(|x| x.as_array())
    {
        for item in c {
            if let Some(s) = item.as_str() {
                cwe.push(s.to_string());
            }
        }
    }

    let category = r
        .get("properties")
        .and_then(|p| p.get("category"))
        .and_then(|x| x.as_str())
        .map(|s| s.to_string());

    // Location: prefer `locations[0].physicalLocation`.
    let mut path = None;
    let mut line = None;
    if let Some(locs) = r.get("locations").and_then(|x| x.as_array())
        && let Some(first) = locs.first()
    {
        let pl = first.get("physicalLocation");
        path = pl
            .and_then(|p| p.get("artifactLocation"))
            .and_then(|a| a.get("uri"))
            .and_then(|x| x.as_str())
            .map(|s| s.to_string());
        line = pl
            .and_then(|p| p.get("region"))
            .and_then(|rg| rg.get("startLine"))
            .and_then(|x| x.as_u64())
            .map(|u| u as u32);
    }

    // Code flows (taint-style evidence) serialized to strings for portability.
    let mut code_flows = Vec::new();
    if let Some(cf) = r.get("codeFlows").and_then(|x| x.as_array()) {
        for flow in cf {
            if let Ok(s) = serde_json::to_string(flow) {
                code_flows.push(s);
            }
        }
    }

    SarifResult {
        rule_id,
        level,
        message,
        rule_index,
        cwe,
        category,
        path,
        line,
        code_flows,
    }
}

fn sarif_level_to_severity(level: &str) -> String {
    match level {
        "error" => "error",
        "warning" => "warning",
        "note" => "info",
        "none" => "info",
        _ => "warning",
    }
    .to_string()
}

/// Re-export of the minimal YAML parser so callers needing protocol files
/// alongside SARIF can share the dependency-free loader. (Keeps the public
/// surface coherent: `sarif` + `rules` are both "external input" modules.)
pub use crate::rules::parse_yaml;

/// Build a normalized issue map keyed by `tool:path:line:rule_id` for dedupe.
/// The `tool` component keeps findings from different analyzers (e.g. taint vs
/// SCA) at the same location distinct, while still collapsing exact duplicates
/// from the same tool.
pub fn issue_dedup_key(issue: &SecurityIssue) -> String {
    format!(
        "{}:{}:{}:{}",
        issue.tool,
        issue.path.as_deref().unwrap_or("-"),
        issue.line.unwrap_or(0),
        issue.rule_id
    )
}
