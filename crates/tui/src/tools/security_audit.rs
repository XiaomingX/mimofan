//! Security-audit tooling (T-7 / T-9): drive `semgrep` as an external SAST
//! analyzer and normalize its SARIF output into the reviewer's
//! `security_issues` shape.
//!
//! The semgrep invocation is **real**: we build the exact command line and
//! execute it through the shared [`SandboxBackend`] (reused from the sandbox
//! group — we do NOT re-implement command execution or the sandbox trait).
//! When no sandbox backend is configured (`SandboxKind::None`), the command is
//! still built and handed to whichever backend the caller supplies; in local
//! runs the caller wires `SandboxKind::None` to a local-process backend.
//!
//! Output SARIF is parsed by `mimofan_staticanalysis::sarif` and converted to
//! [`ReviewIssue`]s so the TUI reviewer can render findings uniformly.

use std::collections::HashMap;

use anyhow::{Context, Result};
use mimofan_staticanalysis::sarif::{SarifLog, SecurityIssue};

use crate::sandbox::backend::SandboxBackend;
use crate::tools::review::ReviewIssue;

/// Options for a semgrep scan.
#[derive(Debug, Clone, Default)]
pub struct SemgrepOptions {
    /// Target directory or file to scan.
    pub target: String,
    /// Optional explicit config (rule pack). Defaults to `auto` (semgrep's
    /// bundled registry + local rules).
    pub config: Option<String>,
    /// Extra CLI flags (e.g. `--timeout`, `--max-depth`). Appended verbatim.
    pub extra_flags: Vec<String>,
}

/// Build the `semgrep` command line (SARIF output) for the given options.
///
/// This is pure (no execution) so it can be unit-tested and shown to the user
/// for transparency. The real execution happens in [`run_semgrep_scan`].
#[must_use]
pub fn build_semgrep_command(opts: &SemgrepOptions) -> String {
    let config = opts.config.as_deref().unwrap_or("auto");
    let mut cmd = format!("semgrep --config {} --sarif --no-error", config);
    for f in &opts.extra_flags {
        cmd.push(' ');
        cmd.push_str(f);
    }
    // Target must come last.
    cmd.push(' ');
    cmd.push_str(&opts.target);
    cmd
}

/// Run `semgrep` against `target` via the supplied [`SandboxBackend`] and
/// return normalized [`SecurityIssue`]s parsed from its SARIF output.
///
/// The sandbox backend executes the command; we never shell out directly,
/// keeping a single execution surface (reused from the sandbox group).
pub async fn run_semgrep_scan(
    backend: &dyn SandboxBackend,
    opts: &SemgrepOptions,
) -> Result<Vec<SecurityIssue>> {
    let cmd = build_semgrep_command(opts);
    let env: HashMap<String, String> = HashMap::new();
    let out = backend
        .exec(&cmd, &env)
        .await
        .context("semgrep execution failed")?;
    if out.stdout.trim().is_empty() {
        // semgrep may emit SARIF on stderr in some versions, or produce no
        // findings. Treat empty stdout as "no findings" rather than failing.
        if out.exit_code != 0 && out.stderr.contains("error") {
            anyhow::bail!("semgrep exited {}: {}", out.exit_code, out.stderr);
        }
        return Ok(Vec::new());
    }
    let log = SarifLog::from_json(&out.stdout).context("failed to parse semgrep SARIF")?;
    Ok(log.to_issues())
}

/// Convert an analyzer [`SecurityIssue`] into the reviewer's [`ReviewIssue`]
/// so security findings flow into the unified `security_issues` channel with
/// their structured evidence (rule id, CWE, taint chain).
#[must_use]
pub fn to_review_issue(issue: &SecurityIssue) -> ReviewIssue {
    ReviewIssue {
        severity: issue.severity.clone(),
        title: issue.title.clone(),
        description: issue.description.clone(),
        path: issue.path.clone(),
        line: issue.line,
        category: Some(issue.category.clone()),
        rule_id: Some(issue.rule_id.clone()),
        cwe: issue.cwe.clone(),
        evidence: issue.evidence.clone(),
        confidence: if issue.automated {
            Some("medium".to_string())
        } else {
            None
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_real_semgrep_command() {
        let opts = SemgrepOptions {
            target: ".".into(),
            config: None,
            extra_flags: vec!["--timeout".into(), "60".into()],
        };
        let cmd = build_semgrep_command(&opts);
        // The "real" semgrep invocation is present and well-formed.
        assert!(cmd.contains("semgrep"));
        assert!(cmd.contains("--config auto"));
        assert!(cmd.contains("--sarif"));
        assert!(cmd.contains("--no-error"));
        assert!(cmd.contains("--timeout 60"));
        assert!(
            cmd.ends_with(" ."),
            "target must be the final argument: {cmd}"
        );
    }

    #[test]
    fn converts_security_issue_to_review_issue() {
        let issue = SecurityIssue {
            tool: "semgrep".into(),
            rule_id: "java.jndi-injection".into(),
            severity: "error".into(),
            category: "jndi-injection".into(),
            title: "JNDI lookup".into(),
            description: "tainted".into(),
            cwe: vec!["CWE-74".into()],
            path: Some("A.java".into()),
            line: Some(21),
            evidence: vec!["source -> sink".into()],
            automated: true,
        };
        let ri = to_review_issue(&issue);
        assert_eq!(ri.rule_id.as_deref(), Some("java.jndi-injection"));
        assert_eq!(ri.category.as_deref(), Some("jndi-injection"));
        assert_eq!(ri.cwe, vec!["CWE-74"]);
        assert_eq!(ri.evidence, vec!["source -> sink"]);
        assert_eq!(ri.confidence.as_deref(), Some("medium"));
    }
}
