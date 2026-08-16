//! #853 — Unattended safety subset.
//!
//! `UnattendedPolicy` restricts the engine to a SAFE subset of tools suitable
//! for fully headless runs: no tools requiring human approval, no destructive
//! operations (file writes, code execution), and no network egress. The engine
//! runs to completion without ever blocking on input.
//!
//! The policy is a pure filter over the tool registry — it never mutates the
//! underlying tools, only decides which names are permitted in unattended
//! mode. It is applied at turn-registry construction time (see
//! `crate::core::engine`) so the model only ever sees the safe surface.

use crate::tools::spec::{ApprovalRequirement, ToolSpec};
use mimofan_tools::ToolCapability as Cap;
use std::sync::Arc;

use crate::tools::registry::ToolRegistry;

/// A tool is permitted in unattended mode only when it is both *safe* and
/// *non-blocking*:
///
/// - **Non-blocking**: its `approval_requirement()` is `Auto` (never requires a
///   human in the loop). `Suggest`/`Required` tools are excluded because they
///   would otherwise stall waiting for input that never arrives.
/// - **Safe**: it must be read-only (`ReadOnly` capability) and must NOT carry
///   any of the dangerous capabilities — `WritesFiles`, `ExecutesCode`, or
///   `Network` (egress). Destructive or egress-capable tools are excluded even
///   if they happen to be auto-approved.
fn is_unattended_safe(spec: &dyn ToolSpec) -> bool {
    if spec.approval_requirement() != ApprovalRequirement::Auto {
        return false;
    }
    if !spec.is_read_only() {
        return false;
    }
    let caps = spec.capabilities();
    if caps.contains(&Cap::WritesFiles)
        || caps.contains(&Cap::ExecutesCode)
        || caps.contains(&Cap::Network)
    {
        return false;
    }
    true
}

/// Policy that filters a tool registry down to the unattended-safe subset.
#[derive(Debug, Clone, Default)]
pub struct UnattendedPolicy {
    /// When `false` the policy is a no-op (returns every tool as-is). Set to
    /// `true` to actually restrict to the safe subset.
    enabled: bool,
}

impl UnattendedPolicy {
    /// Create a policy in the given enabled state.
    #[must_use]
    pub fn new(enabled: bool) -> Self {
        Self { enabled }
    }

    /// Whether the policy will actually filter tools.
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Return the names of tools permitted under this policy from a registry.
    ///
    /// When disabled, every registered tool name is returned (preserving the
    /// existing interactive behavior). When enabled, only tools passing
    /// [`is_unattended_safe`] are returned. Names are owned `String`s so the
    /// result does not borrow the (often temporary) registry snapshot.
    #[must_use]
    pub fn allowed_tool_names(&self, registry: &ToolRegistry) -> Vec<String> {
        if !self.enabled {
            return registry.names().into_iter().map(str::to_string).collect();
        }
        let all = registry.all();
        all.iter()
            .filter(|t| is_unattended_safe(t.as_ref()))
            .map(|t| t.name().to_string())
            .collect()
    }

    /// Return the safe-subset tool specs from a registry (when enabled) or all
    /// specs (when disabled).
    #[must_use]
    pub fn allowed_tools(&self, registry: &ToolRegistry) -> Vec<Arc<dyn ToolSpec>> {
        if !self.enabled {
            return registry.all();
        }
        registry
            .all()
            .into_iter()
            .filter(|t| is_unattended_safe(t.as_ref()))
            .collect()
    }

    /// Whether a single tool spec is permitted under this policy.
    #[must_use]
    pub fn is_allowed(&self, spec: &dyn ToolSpec) -> bool {
        if !self.enabled {
            return true;
        }
        is_unattended_safe(spec)
    }
}

/// Capability-set helper: whether a tool is purely read-only and free of any
/// dangerous capability. Exposed for unit tests and reuse.
#[must_use]
pub fn tool_is_unattended_safe(spec: &dyn ToolSpec) -> bool {
    is_unattended_safe(spec)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::spec::{ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec};
    use mimofan_tools::ToolCapability as Cap;
    use serde_json::Value;

    /// A hand-rolled `ToolSpec` used to exercise the `UnattendedPolicy` filter.
    struct TestTool {
        name: String,
        approval: ApprovalRequirement,
        caps: Vec<Cap>,
    }

    #[async_trait::async_trait]
    impl ToolSpec for TestTool {
        fn name(&self) -> &str {
            &self.name
        }
        fn description(&self) -> &str {
            "test tool"
        }
        fn input_schema(&self) -> Value {
            Value::Object(serde_json::Map::new())
        }
        fn capabilities(&self) -> Vec<ToolCapability> {
            self.caps.clone()
        }
        fn approval_requirement(&self) -> ApprovalRequirement {
            self.approval
        }
        async fn execute(
            &self,
            _input: Value,
            _ctx: &ToolContext,
        ) -> Result<ToolResult, ToolError> {
            Ok(ToolResult::success("ok".to_string()))
        }
    }

    fn read_only_auto(name: &str) -> Arc<dyn ToolSpec> {
        Arc::new(TestTool {
            name: name.to_string(),
            approval: ApprovalRequirement::Auto,
            caps: vec![Cap::ReadOnly],
        })
    }

    fn write_tool(name: &str) -> Arc<dyn ToolSpec> {
        Arc::new(TestTool {
            name: name.to_string(),
            approval: ApprovalRequirement::Auto,
            caps: vec![Cap::WritesFiles],
        })
    }

    fn shell_tool(name: &str) -> Arc<dyn ToolSpec> {
        Arc::new(TestTool {
            name: name.to_string(),
            approval: ApprovalRequirement::Auto,
            caps: vec![Cap::ExecutesCode],
        })
    }

    fn network_tool(name: &str) -> Arc<dyn ToolSpec> {
        Arc::new(TestTool {
            name: name.to_string(),
            approval: ApprovalRequirement::Auto,
            caps: vec![Cap::Network],
        })
    }

    fn human_approval_tool(name: &str) -> Arc<dyn ToolSpec> {
        Arc::new(TestTool {
            name: name.to_string(),
            approval: ApprovalRequirement::Required,
            caps: vec![Cap::ReadOnly],
        })
    }

    fn make_registry() -> ToolRegistry {
        let mut reg = ToolRegistry::new(ToolContext::new(
            std::env::temp_dir().join("mimofan_unattended_test_ws"),
        ));
        reg.register_all(vec![
            read_only_auto("read_file"),
            write_tool("write_file"),
            shell_tool("exec_shell"),
            network_tool("web_search"),
            human_approval_tool("revert_turn"),
        ]);
        reg
    }

    #[test]
    fn disabled_policy_returns_everything() {
        let reg = make_registry();
        let policy = UnattendedPolicy::new(false);
        let allowed = policy.allowed_tool_names(&reg);
        assert_eq!(allowed.len(), 5, "disabled policy is a no-op");
    }

    #[test]
    fn acceptance_859_unattended_excludes_blocking_and_destructive() {
        // #859 — the tool permission boundary must be enforced: a tool whose
        // approval requirement needs a human (Required / AskHuman) is EXCLUDED,
        // and any tool flagged WritesFiles / ExecutesCode / Network is EXCLUDED
        // even when auto-approved. Only ReadOnly + Auto tools survive.
        let reg = make_registry();
        let policy = UnattendedPolicy::new(true);

        // Surviving set is exactly the read-only auto tool.
        let allowed = policy.allowed_tool_names(&reg);
        assert_eq!(
            allowed,
            vec!["read_file".to_string()],
            "only the read-only auto tool survives the boundary"
        );

        // A tool requiring human approval (even though read-only) is dropped.
        assert!(
            !policy.is_allowed(human_approval_tool("revert_turn").as_ref()),
            "human-approval (Required/AskHuman) tool must be excluded"
        );
        // Suggest approval also blocks — it is not Auto.
        let suggest = TestTool {
            name: "maybe_approve".to_string(),
            approval: ApprovalRequirement::Suggest,
            caps: vec![Cap::ReadOnly],
        };
        assert!(
            !policy.is_allowed(&suggest),
            "Suggest-approval tool must be excluded (only Auto survives)"
        );
        // Destructive / egress-capable tools are dropped even when auto-approved.
        assert!(
            !policy.is_allowed(write_tool("write_file").as_ref()),
            "WritesFiles tool must be excluded"
        );
        assert!(
            !policy.is_allowed(shell_tool("exec_shell").as_ref()),
            "ExecutesCode tool must be excluded"
        );
        assert!(
            !policy.is_allowed(network_tool("web_search").as_ref()),
            "Network tool must be excluded"
        );
        // The read-only auto tool is retained.
        assert!(
            policy.is_allowed(read_only_auto("read_file").as_ref()),
            "ReadOnly + Auto tool must be retained"
        );
    }

    #[test]
    fn enabled_policy_keeps_only_read_only_auto_tools() {
        let reg = make_registry();
        let policy = UnattendedPolicy::new(true);
        let allowed = policy.allowed_tool_names(&reg);
        assert_eq!(
            allowed,
            vec!["read_file".to_string()],
            "only the safe read-only tool survives"
        );
        // A tool requiring human approval is excluded even though read-only.
        assert!(!policy.is_allowed(human_approval_tool("revert_turn").as_ref()));
        // Destructive / egress tools are excluded even when auto-approved.
        assert!(!policy.is_allowed(write_tool("write_file").as_ref()));
        assert!(!policy.is_allowed(shell_tool("exec_shell").as_ref()));
        assert!(!policy.is_allowed(network_tool("web_search").as_ref()));
    }

    /// #859 acceptance: prove the permission boundary precisely. Under an
    /// enabled unattended policy, the safe subset is exactly the tools that are
    /// (a) `Auto`-approved and (b) read-only AND carry none of the dangerous
    /// capabilities `WritesFiles` / `ExecutesCode` / `Network`. Every other
    /// tool is dropped — including read-only tools that merely require human
    /// approval, and destructive/egress tools that happen to be auto-approved.
    #[test]
    fn acceptance_859_permission_boundary_is_precise() {
        let reg = make_registry();
        let policy = UnattendedPolicy::new(true);

        // The boundary MUST retain: ReadOnly + Auto.
        assert!(
            policy.is_allowed(read_only_auto("read_file").as_ref()),
            "ReadOnly + Auto tool must survive"
        );

        // The boundary MUST drop, by precise reason:
        // 1. ReadOnly but human-approval-required (would block headless).
        assert!(
            !policy.is_allowed(human_approval_tool("revert_turn").as_ref()),
            "ReadOnly but AskHuman/Required must be excluded"
        );
        // 2. WritesFiles even if auto-approved.
        assert!(
            !policy.is_allowed(write_tool("write_file").as_ref()),
            "WritesFiles must be excluded"
        );
        // 3. ExecutesCode even if auto-approved.
        assert!(
            !policy.is_allowed(shell_tool("exec_shell").as_ref()),
            "ExecutesCode must be excluded"
        );
        // 4. Network egress even if auto-approved.
        assert!(
            !policy.is_allowed(network_tool("web_search").as_ref()),
            "Network must be excluded"
        );

        // Net effect: only the single safe ReadOnly+Auto tool survives.
        let allowed = policy.allowed_tool_names(&reg);
        assert_eq!(allowed.len(), 1);
        assert_eq!(allowed[0], "read_file");
    }
}
