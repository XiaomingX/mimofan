//! Attack-surface enumeration tool: `attack_surface`.
//!
//! Wires the [`mimofan_staticanalysis::attack_surface`] engine (T-12) and the
//! KB reverse-tracer ([`mimofan_staticanalysis::kb_trace`]) into the tool
//! surface, giving the model a *dependency-derived* attack-surface enumeration:
//! satisfied gadget chains, implicit unsafe `autoType` deserialization, and
//! known-vulnerable dependencies (OSV). This is the AVDH **Threat Model /
//! Entry Point Discovery** primitive (plan `plans/12` Phase 2) — the engine
//! existed with ZERO TUI callers before this tool.
//!
//! Unlike `auto_gadget_discovery` (which discovers *source-level* sink/pivot
//! symbols) and `gadget_chain_trace` (which reverse-traces from a model-supplied
//! sink), this tool enumerates the *dependency-driven* attack surface from a
//! lockfile. It is read-only and offline: OSV advisories come from an in-memory
//! client that returns no results unless a caller has seeded them (tests),
//! mirroring the crate's test-only network posture.

use std::path::Path;

use async_trait::async_trait;
use mimofan_staticanalysis::attack_surface::{
    AttackSurfaceEntry, AttackSurfaceKind, scan_attack_surface,
};
use mimofan_staticanalysis::knowledge::load_kb_dir;
use mimofan_staticanalysis::sarif::SecurityIssue;
use mimofan_staticanalysis::sca::InMemoryOsv;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::review::ReviewIssue;
use super::security_audit::to_review_issue;
use super::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec,
};

/// Tool name for the model-facing API.
pub const ATTACK_SURFACE_TOOL_NAME: &str = "attack_surface";

/// On-disk KB location, resolved relative to the staticanalysis crate (same
/// path the `gadget_chain_trace` / `auto_gadget_discovery` tools use).
const KB_DIR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../staticanalysis/src/rules/kb"
);

/// Recognized lockfile file names, in priority order.
const LOCKFILES: &[&str] = &["Cargo.lock", "package-lock.json", "yarn.lock"];

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AttackSurfaceInput {
    /// Directory to enumerate. The tool looks for a supported lockfile
    /// (`Cargo.lock`, `package-lock.json`, `yarn.lock`) directly inside it.
    target_dir: String,
    /// Optional explicit lockfile path (relative to the workspace). When
    /// omitted, the first supported lockfile found in `target_dir` is used.
    #[serde(default)]
    lockfile: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AttackSurfaceEntryOut {
    kind: String,
    severity: String,
    category: String,
    title: String,
    detail: String,
    ref_id: Option<String>,
    references: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AttackSurfaceOutput {
    target_dir: String,
    lockfile: Option<String>,
    kb_loaded: bool,
    entry_count: usize,
    entries: Vec<AttackSurfaceEntryOut>,
    /// Normalized findings for the unified `security_issues` channel.
    findings: Vec<ReviewIssue>,
}

/// Enumerate the dependency-derived attack surface of a target directory.
pub struct AttackSurfaceTool;

impl AttackSurfaceTool {
    /// Core logic shared by `execute` and the unit test.
    fn run_enumeration(input: &AttackSurfaceInput) -> Result<AttackSurfaceOutput, ToolError> {
        let target = Path::new(&input.target_dir);
        if !target.exists() {
            return Err(ToolError::invalid_input(format!(
                "target_dir does not exist: {}",
                input.target_dir
            )));
        }

        // Resolve the lockfile: explicit path, or auto-discover in target_dir.
        let lock_path = match &input.lockfile {
            Some(lf) => lf.clone(),
            None => LOCKFILES
                .iter()
                .find(|name| target.join(name).is_file())
                .map(|name| name.to_string())
                .ok_or_else(|| {
                    ToolError::invalid_input(
                        "no supported lockfile (Cargo.lock / package-lock.json / yarn.lock) \
                         found in target_dir; supply 'lockfile' explicitly",
                    )
                })?,
        };
        let lock_content = std::fs::read_to_string(target.join(&lock_path)).map_err(|e| {
            ToolError::execution_failed(format!("failed to read {lock_path}: {e}"))
        })?;

        // Offline OSV: no network. In-memory client returns no advisories
        // unless seeded (tests). Keeps the tool hermetic and read-only.
        let osv = InMemoryOsv::default();

        let kb = match load_kb_dir(KB_DIR) {
            Ok(kb) => kb,
            Err(e) => {
                return Err(ToolError::execution_failed(format!("load KB: {e}")));
            }
        };
        let kb_loaded = !kb.is_empty();

        let entries = scan_attack_surface(
            &kb,
            target.join(&lock_path).to_str().unwrap_or(&lock_path),
            &lock_content,
            &osv,
        )
        .map_err(|e| ToolError::execution_failed(format!("attack-surface scan failed: {e}")))?;

        // Normalize each entry into a SecurityIssue then into the unified
        // ReviewIssue channel (same shape `security_audit` emits).
        let mut findings: Vec<ReviewIssue> = Vec::new();
        let out_entries: Vec<AttackSurfaceEntryOut> = entries
            .into_iter()
            .map(|e| {
                findings.push(to_review_issue(&entry_to_security_issue(&e)));
                AttackSurfaceEntryOut {
                    kind: kind_name(&e.kind),
                    severity: e.severity,
                    category: e.category,
                    title: e.title,
                    detail: e.detail,
                    ref_id: e.ref_id,
                    references: e.references,
                }
            })
            .collect();

        Ok(AttackSurfaceOutput {
            target_dir: input.target_dir.clone(),
            lockfile: Some(lock_path),
            kb_loaded,
            entry_count: out_entries.len(),
            entries: out_entries,
            findings,
        })
    }
}

/// Map an `AttackSurfaceKind` to a stable string label.
fn kind_name(kind: &AttackSurfaceKind) -> String {
    match kind {
        AttackSurfaceKind::GadgetChain => "gadget-chain".to_string(),
        AttackSurfaceKind::ImplicitAutoType => "implicit-autotype".to_string(),
        AttackSurfaceKind::VulnerableDependency => "vulnerable-dependency".to_string(),
        AttackSurfaceKind::SinkPresent => "sink-present".to_string(),
    }
}

/// Convert an `AttackSurfaceEntry` into the unified [`SecurityIssue`] shape so
/// it can flow through the same normalization as `security_audit` findings.
fn entry_to_security_issue(entry: &AttackSurfaceEntry) -> SecurityIssue {
    SecurityIssue {
        tool: ATTACK_SURFACE_TOOL_NAME.to_string(),
        rule_id: entry.ref_id.clone().unwrap_or_else(|| entry.category.clone()),
        severity: entry.severity.clone(),
        category: entry.category.clone(),
        title: entry.title.clone(),
        description: entry.detail.clone(),
        cwe: entry
            .references
            .iter()
            .filter(|r| r.to_ascii_uppercase().starts_with("CWE"))
            .cloned()
            .collect(),
        path: None,
        line: None,
        evidence: Vec::new(),
        automated: true,
    }
}

#[async_trait]
impl ToolSpec for AttackSurfaceTool {
    fn name(&self) -> &'static str {
        ATTACK_SURFACE_TOOL_NAME
    }

    fn description(&self) -> &'static str {
        "Enumerate the dependency-derived attack surface of a target directory: \
         satisfied gadget chains, implicit unsafe autoType deserialization, and \
         known-vulnerable dependencies (from a lockfile). Provide `target_dir` \
         (a directory containing Cargo.lock / package-lock.json / yarn.lock) or \
         an explicit `lockfile` path. Read-only and offline."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "target_dir": {
                    "type": "string",
                    "description": "Directory to enumerate. Must contain a supported lockfile unless 'lockfile' is supplied."
                },
                "lockfile": {
                    "type": "string",
                    "description": "Optional explicit lockfile path (relative to the workspace). Default: auto-discover in target_dir."
                }
            },
            "required": ["target_dir"],
            "additionalProperties": false
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    fn supports_parallel(&self) -> bool {
        true
    }

    async fn execute(&self, input: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        let parsed: AttackSurfaceInput = serde_json::from_value(input.clone()).map_err(|e| {
            ToolError::invalid_input(format!("invalid attack_surface input: {e}"))
        })?;

        // Resolve the target_dir against the workspace root so callers can pass
        // a workspace-relative path.
        let resolved_dir = context
            .resolve_path(&parsed.target_dir)
            .map(|p| p.display().to_string())
            .map_err(|_| {
                ToolError::invalid_input(format!(
                    "invalid target_dir: {}",
                    parsed.target_dir
                ))
            })?;

        let mut normalized = parsed;
        normalized.target_dir = resolved_dir;

        let output = Self::run_enumeration(&normalized)?;
        ToolResult::json(&output).map_err(|e| ToolError::execution_failed(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    const CARGO_LOCK: &str = r#"
[[package]]
name = "foo"
version = "0.1.0"
"#;

    #[test]
    fn rejects_missing_target() {
        let err = AttackSurfaceTool::run_enumeration(&AttackSurfaceInput {
            target_dir: "/nonexistent/definitely/not/here".into(),
            lockfile: None,
        })
        .expect_err("must reject a missing target_dir");
        assert!(
            err.to_string().contains("does not exist"),
            "error must mention missing target, got: {err}"
        );
    }

    #[test]
    fn rejects_dir_without_lockfile() {
        let dir = TempDir::new().expect("tempdir");
        let err = AttackSurfaceTool::run_enumeration(&AttackSurfaceInput {
            target_dir: dir.path().display().to_string(),
            lockfile: None,
        })
        .expect_err("must reject a directory without a supported lockfile");
        assert!(
            err.to_string().contains("no supported lockfile"),
            "error must mention lockfile, got: {err}"
        );
    }

    #[test]
    fn enumerates_from_cargo_lock() {
        let dir = TempDir::new().expect("tempdir");
        let mut f = std::fs::File::create(dir.path().join("Cargo.lock")).expect("create lock");
        f.write_all(CARGO_LOCK.as_bytes()).expect("write lock");
        f.flush().unwrap();

        let out = AttackSurfaceTool::run_enumeration(&AttackSurfaceInput {
            target_dir: dir.path().display().to_string(),
            lockfile: None,
        })
        .expect("enumeration should succeed");
        assert_eq!(out.lockfile.as_deref(), Some("Cargo.lock"));
        assert!(out.kb_loaded, "the bundled KB should load");
        // A single harmless dep yields no gadget chain / autotype / advisory.
        assert_eq!(out.entry_count, 0, "a clean lockfile yields no findings");
        assert!(out.findings.is_empty());
    }

    #[test]
    fn explicit_lockfile_path_is_used() {
        let dir = TempDir::new().expect("tempdir");
        // Place the lockfile in a subdir: auto-discovery only scans the
        // top-level target_dir, so only an explicit 'lockfile' path reaches it.
        let sub = dir.path().join("sub");
        std::fs::create_dir_all(&sub).expect("create subdir");
        let mut f = std::fs::File::create(sub.join("Cargo.lock")).expect("create lock");
        f.write_all(CARGO_LOCK.as_bytes()).expect("write lock");
        f.flush().unwrap();

        let out = AttackSurfaceTool::run_enumeration(&AttackSurfaceInput {
            target_dir: dir.path().display().to_string(),
            lockfile: Some("sub/Cargo.lock".into()),
        })
        .expect("explicit lockfile should be honored");
        assert_eq!(out.lockfile.as_deref(), Some("sub/Cargo.lock"));
    }
}
