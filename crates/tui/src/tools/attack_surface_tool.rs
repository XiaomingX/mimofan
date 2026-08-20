//! Attack-surface enumeration tool: `attack_surface`.
//!
//! Wraps the offline `staticanalysis::attack_surface::scan_attack_surface`
//! engine (which ties the curated [`KnowledgeBase`] to the project's resolved
//! dependencies and offline OSV advisories) so the model can enumerate a
//! project's attack surface — satisfied gadget chains, implicit autoType
//! deserialization configs, and known-vulnerable dependencies. The engine is
//! synchronous and read-only, so the heavy scan runs inside `spawn_blocking`
//! and the tool is registered `Auto` / parallel-safe.
//!
//! The underlying engine code in `crates/staticanalysis` is reused as-is
//! (never modified): this file only performs the thin TUI wrapper.
use async_trait::async_trait;
use mimofan_staticanalysis::attack_surface::{AttackSurfaceEntry, AttackSurfaceKind};
use mimofan_staticanalysis::knowledge::load_kb_dir;
use mimofan_staticanalysis::sca::InMemoryOsv;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec,
};

/// Tool name for the model-facing API.
pub const ATTACK_SURFACE_TOOL_NAME: &str = "attack_surface";

/// On-disk KB location, resolved relative to the staticanalysis crate. The tui
/// crate sits at `crates/tui`, so `../staticanalysis` reaches the sibling crate
/// whose `src/rules/kb` holds the curated knowledge base.
const KB_DIR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../staticanalysis/src/rules/kb"
);

/// Enumerate a project's attack surface from its lockfile + the offline KB.
pub struct AttackSurfaceTool;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AttackSurfaceInput {
    /// Root directory to scan. Required.
    #[serde(default)]
    target_dir: String,
    /// Optional lockfile path relative to `target_dir` (e.g.
    /// `Cargo.lock` / `package-lock.json`). Defaults to `target_dir/Cargo.lock`.
    #[serde(default)]
    lockfile: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AttackSurfaceEntryView {
    kind: String,
    title: String,
    severity: String,
    category: String,
    detail: String,
    ref_id: Option<String>,
    references: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AttackSurfaceOutput {
    target_dir: String,
    entries: Vec<AttackSurfaceEntryView>,
}

/// Human-readable tag for an [`AttackSurfaceKind`].
fn kind_str(kind: &AttackSurfaceKind) -> &'static str {
    match kind {
        AttackSurfaceKind::GadgetChain => "gadget_chain",
        AttackSurfaceKind::ImplicitAutoType => "implicit_auto_type",
        AttackSurfaceKind::VulnerableDependency => "vulnerable_dependency",
        AttackSurfaceKind::SinkPresent => "sink_present",
    }
}

fn to_view(entry: &AttackSurfaceEntry) -> AttackSurfaceEntryView {
    AttackSurfaceEntryView {
        kind: kind_str(&entry.kind).to_string(),
        title: entry.title.clone(),
        severity: entry.severity.clone(),
        category: entry.category.clone(),
        detail: entry.detail.clone(),
        ref_id: entry.ref_id.clone(),
        references: entry.references.clone(),
    }
}

#[async_trait]
impl ToolSpec for AttackSurfaceTool {
    fn name(&self) -> &'static str {
        ATTACK_SURFACE_TOOL_NAME
    }

    fn description(&self) -> &'static str {
        "Enumerate a project's attack surface offline: satisfied gadget chains, \
         implicit autoType deserialization configs, and known-vulnerable dependencies \
         (from the curated KB + the project lockfile, using an in-memory offline OSV \
         client). Read-only: no code runs, no files are written, no network is used."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "target_dir": {
                    "type": "string",
                    "description": "Root directory to scan. Required."
                },
                "lockfile": {
                    "type": "string",
                    "description": "Optional lockfile path relative to target_dir (e.g. Cargo.lock / package-lock.json). Defaults to target_dir/Cargo.lock."
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

    async fn execute(&self, input: Value, _context: &ToolContext) -> Result<ToolResult, ToolError> {
        let parsed: AttackSurfaceInput = serde_json::from_value(input.clone()).map_err(|e| {
            ToolError::invalid_input(format!("invalid attack_surface input: {e}"))
        })?;

        if parsed.target_dir.trim().is_empty() {
            return Err(ToolError::invalid_input(
                "attack_surface requires a non-empty 'target_dir'",
            ));
        }

        // Resolve the lockfile path (default: target_dir/Cargo.lock). Reading is
        // best-effort: a missing lockfile is non-fatal and yields no entries.
        let lock_rel = parsed.lockfile.as_deref().unwrap_or("Cargo.lock");
        let lock_path = std::path::Path::new(&parsed.target_dir).join(lock_rel);
        let lock_path_str = lock_path.to_string_lossy().to_string();
        let lock_content = std::fs::read_to_string(&lock_path).ok();

        // Load the KB and run the sync scan off the async executor.
        let kb_dir = KB_DIR.to_string();
        let lock_path = lock_path_str.clone();
        let lock_content = lock_content.clone();
        let scan: Result<Vec<AttackSurfaceEntry>, anyhow::Error> =
            tokio::task::spawn_blocking(move || {
                let kb = load_kb_dir(&kb_dir)?;
                let osv = InMemoryOsv::default();
                scan_attack_surface_impl(&kb, &lock_path, lock_content.as_deref(), &osv)
            })
            .await
            .map_err(|e| ToolError::execution_failed(format!("task join: {e}")))?;

        let entries = scan
            .map_err(|e| ToolError::execution_failed(format!("attack_surface scan: {e}")))?;
        let views: Vec<AttackSurfaceEntryView> = entries.iter().map(to_view).collect();
        let output = AttackSurfaceOutput {
            target_dir: parsed.target_dir,
            entries: views,
        };
        ToolResult::json(&output).map_err(|e| ToolError::execution_failed(e.to_string()))
    }
}

/// Thin wrapper around `scan_attack_surface`: an empty (missing) lockfile yields
/// an empty entry list instead of erroring, so the tool degrades gracefully.
fn scan_attack_surface_impl(
    kb: &mimofan_staticanalysis::knowledge::KnowledgeBase,
    lock_path: &str,
    lock_content: Option<&str>,
    osv: &dyn mimofan_staticanalysis::sca::OsvClient,
) -> Result<Vec<AttackSurfaceEntry>, anyhow::Error> {
    match lock_content {
        Some(content) => {
            mimofan_staticanalysis::attack_surface::scan_attack_surface(kb, lock_path, content, osv)
        }
        None => Ok(Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_is_attack_surface() {
        let tool = AttackSurfaceTool;
        assert_eq!(tool.name(), "attack_surface");
    }

    #[test]
    fn input_schema_requires_target_dir() {
        let tool = AttackSurfaceTool;
        let schema = tool.input_schema();
        let required = schema["required"].as_array().expect("required array");
        assert!(
            required.iter().any(|v| v.as_str() == Some("target_dir")),
            "input schema must require 'target_dir'"
        );
        assert!(
            schema["properties"]["target_dir"]["type"].as_str() == Some("string"),
            "target_dir must be a string property"
        );
    }
}
