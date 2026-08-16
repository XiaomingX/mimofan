//! Plugin capability assembly (issue #834, plan W1).
//!
//! Given a [`PluginManifest`], [`assemble`] collects the concrete capability
//! objects that should be injected into the running agent. W1 only populates
//! the `tools` slice by resolving the `extra` tool names against a registry of
//! known extra tools; `sandbox` and `llm` are declared (so the
//! `AssembledCapabilities` type is stable across workstreams) but left `None`
//! until W2/W4 implement their assembly.

use std::sync::Arc;

use tracing::warn;

use crate::client::ApiClient;
use crate::sandbox::backend::SandboxBackend;
use crate::tools::gadget_chain::GadgetChainTraceTool;
use crate::tools::hypothesis::HypothesisTool;
use crate::tools::run_poc::RunPocTool;
use crate::tools::spec::ToolSpec;

use super::manifest::PluginManifest;

/// Concrete capabilities resolved from a [`PluginManifest`].
///
/// `sandbox`/`llm` are stabilized here (always present as fields) but are
/// `None` until W2/W4 fill them. `session_events` records whether any plugin
/// requested session-lifecycle event hooks (none in W1).
#[derive(Default)]
pub struct AssembledCapabilities {
    /// Extra tools selected by the manifest's `tools.extra` list.
    pub tools: Vec<Arc<dyn ToolSpec>>,
    /// Resolved sandbox backend (W2).
    pub sandbox: Option<Arc<dyn SandboxBackend>>,
    /// Resolved LLM client (W4). `LlmClient` is not dyn-compatible (async
    /// methods + `impl Trait` returns), so we store the concrete `ApiClient`
    /// — the sole `LlmClient` implementor — behind `Arc`. W4 populates this.
    pub llm: Option<Arc<ApiClient>>,
    /// Whether any plugin registered session-lifecycle event hooks.
    pub session_events: bool,
}

/// Resolve a single `extra` tool name to its concrete tool, if known.
///
/// Returns `None` for unrecognized names (logged via `tracing::warn!` by the
/// caller's [`assemble`] loop).
fn known_extra_tool(name: &str) -> Option<Arc<dyn ToolSpec>> {
    match name {
        crate::tools::hypothesis::HYPOTHESIS_TOOL_NAME => Some(Arc::new(HypothesisTool)),
        crate::tools::gadget_chain::GADGET_CHAIN_TOOL_NAME => Some(Arc::new(GadgetChainTraceTool)),
        crate::tools::run_poc::RUN_POC_TOOL_NAME => Some(Arc::new(RunPocTool)),
        other => {
            warn!(
                tool = other,
                "plugin manifest references unknown extra tool; skipping"
            );
            None
        }
    }
}

/// Assemble capabilities described by `manifest`.
///
/// W1 resolves only `tools.extra`; `sandbox`/`llm` remain `None` and
/// `session_events` is `false`. Unknown `extra` names are skipped (not fatal),
/// preserving legacy behavior when the manifest is empty.
#[must_use]
pub fn assemble(manifest: &PluginManifest) -> AssembledCapabilities {
    let mut tools: Vec<Arc<dyn ToolSpec>> = Vec::new();
    for name in &manifest.tools.extra {
        if let Some(tool) = known_extra_tool(name) {
            tools.push(tool);
        }
    }
    AssembledCapabilities {
        tools,
        sandbox: None,
        llm: None,
        session_events: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::registry::ToolRegistryBuilder;

    #[test]
    fn assemble_resolves_named_extra_tools() {
        let manifest = PluginManifest {
            tools: crate::plugins::manifest::ToolsManifest {
                extra: vec![
                    crate::tools::hypothesis::HYPOTHESIS_TOOL_NAME.to_string(),
                    crate::tools::run_poc::RUN_POC_TOOL_NAME.to_string(),
                ],
            },
        };
        let assembled = assemble(&manifest);
        let names: Vec<&str> = assembled.tools.iter().map(|t| t.name()).collect();
        assert!(names.contains(&crate::tools::hypothesis::HYPOTHESIS_TOOL_NAME));
        assert!(names.contains(&crate::tools::run_poc::RUN_POC_TOOL_NAME));
    }

    #[test]
    fn assemble_empty_manifest_has_no_tools() {
        let manifest = PluginManifest::from_defaults();
        let assembled = assemble(&manifest);
        assert!(assembled.tools.is_empty());
        assert!(assembled.sandbox.is_none());
        assert!(assembled.llm.is_none());
        assert!(!assembled.session_events);
    }

    #[test]
    fn with_extra_tools_registers_in_registry() {
        let dir = std::env::temp_dir().join(format!("mimofan-w1-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp workspace dir");
        let builder = ToolRegistryBuilder::new()
            .with_extra_tools(vec![Arc::new(HypothesisTool)])
            .build(crate::tools::spec::ToolContext::new(dir.clone()));
        let names = builder.names();
        assert!(
            names.contains(&crate::tools::hypothesis::HYPOTHESIS_TOOL_NAME),
            "registry names: {names:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
