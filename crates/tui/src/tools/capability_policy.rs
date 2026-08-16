//! #854 — Tool-level permission policy by capability.
//!
//! `CapabilityPermissionPolicy` is a small, reusable REACTIVE permission
//! boundary over the tool registry: given a set of *denied* capabilities, it
//! decides whether any tool may execute *before* the engine dispatches it.
//!
//! It generalizes the capability-filter idea that `UnattendedPolicy`
//! (#853) introduced for headless mode into a form usable in *any* mode:
//! - `UnattendedPolicy` is a STATIC filter applied at registry-construction
//!   time so the model never even sees a dangerous tool.
//! - `CapabilityPermissionPolicy` is a RUNTIME gate the engine consults right
//!   before executing a tool, so a denied capability fails closed with a
//!   `ToolError::PermissionDenied` even if the tool slipped through the
//!   catalog filter (e.g. an MCP tool, a plugin, or a dynamic tool).
//!
//! The policy is intentionally minimal and additive: it does not mutate the
//! registry, and `deny` is the only lever — `allow_all()` leaves `deny` empty
//! so every tool passes (preserving existing interactive behavior).

use std::collections::HashSet;

use mimofan_tools::ToolCapability;
use std::sync::Arc;

use crate::tools::registry::ToolRegistry;
use crate::tools::spec::ToolSpec;

/// A reactive permission boundary over tools, keyed by [`ToolCapability`].
///
/// A tool is *allowed* iff **none** of its declared capabilities appear in
/// `deny`. This is an deny-list: a tool carrying `ReadOnly` plus a denied
/// capability (e.g. `Network`) is refused the moment any denied capability is
/// present, regardless of how benign its other capabilities are.
#[derive(Debug, Clone, Default)]
pub struct CapabilityPermissionPolicy {
    /// Capabilities that, if present on a tool, cause it to be refused.
    deny: HashSet<ToolCapability>,
    /// Optional hard ceiling on cumulative network egress (bytes) for the
    /// session. `None` means "no network budget enforced". When set, the
    /// engine may consult it to fail closed once the budget is exhausted; the
    /// policy's `is_allowed` gate itself does not track bytes (that is the
    /// caller's job — e.g. `crate::network_policy`), but we carry the field so
    /// the boundary is expressible uniformly and serialisable into config.
    max_network_egress: Option<usize>,
}

impl CapabilityPermissionPolicy {
    /// Create a policy that allows everything (empty deny set).
    ///
    /// This is equivalent to [`allow_all`](Self::allow_all) and preserves the
    /// existing interactive behavior — the gate becomes a no-op.
    #[must_use]
    pub fn new() -> Self {
        Self {
            deny: HashSet::new(),
            max_network_egress: None,
        }
    }

    /// Create a policy that allows every tool (no capability is denied).
    #[must_use]
    pub fn allow_all() -> Self {
        Self::new()
    }

    /// Deny any tool that carries the given capability. Repeated calls
    /// accumulate. Returns `&mut self` for chaining.
    pub fn deny_capability(&mut self, cap: ToolCapability) -> &mut Self {
        self.deny.insert(cap);
        self
    }

    /// Set a hard ceiling on cumulative network egress (bytes) for the
    /// session. `None` disables the budget. Returns `&mut self` for chaining.
    pub fn deny_network_over(&mut self, max_bytes: usize) -> &mut Self {
        self.max_network_egress = Some(max_bytes);
        self
    }

    /// Whether the given capability is currently denied by this policy.
    #[must_use]
    pub fn is_denied(&self, cap: ToolCapability) -> bool {
        self.deny.contains(&cap)
    }

    /// The configured network egress ceiling, if any.
    #[must_use]
    pub fn max_network_egress(&self) -> Option<usize> {
        self.max_network_egress
    }

    /// Runtime gate: whether a tool spec is permitted to execute under this
    /// policy.
    ///
    /// Returns `false` if **any** of the tool's capabilities is in the `deny`
    /// set; `true` otherwise. A tool with no capabilities always passes.
    #[must_use]
    pub fn is_allowed(&self, spec: &dyn ToolSpec) -> bool {
        let caps = spec.capabilities();
        !caps.iter().any(|c| self.deny.contains(c))
    }

    /// Execute the pre-dispatch gate as a fallible check.
    ///
    /// Returns `Ok(())` if the tool may run, or
    /// `Err(ToolError::PermissionDenied(..))` naming the first denied
    /// capability. The engine should call this immediately before
    /// `ToolRegistry::execute*` so a denied tool fails closed instead of
    /// running.
    ///
    /// **Dispatch hook point:** in `crate::core::engine`, before
    /// `registry.execute_full(name, input)` (or `execute_full_with_context`),
    /// resolve the spec via `registry.get(name)` and call
    /// `policy.check_before_execute(spec)`. The engine already holds the
    /// registry, so no signature on `ToolSpec::execute` needs to change.
    #[must_use]
    pub fn check_before_execute(
        &self,
        spec: &dyn ToolSpec,
    ) -> Result<(), crate::tools::spec::ToolError> {
        let caps = spec.capabilities();
        if let Some(denied) = caps.iter().find(|c| self.deny.contains(c)) {
            return Err(crate::tools::spec::ToolError::permission_denied(format!(
                "tool '{}' denied: capability {:?} is not permitted by the active capability policy",
                spec.name(),
                denied
            )));
        }
        Ok(())
    }

    /// Evaluate the whole registry and return the **surviving** tool names
    /// (those not carrying any denied capability), mirroring
    /// `UnattendedPolicy::allowed_tool_names`.
    ///
    /// Names are owned `String`s so the result does not borrow the (often
    /// temporary) registry snapshot. The order is unspecified beyond being
    /// stable for a given registry state.
    #[must_use]
    pub fn allowed_tool_names(&self, registry: &ToolRegistry) -> Vec<String> {
        registry
            .all()
            .iter()
            .filter(|t| self.is_allowed(t.as_ref()))
            .map(|t| t.name().to_string())
            .collect()
    }

    /// Evaluate the whole registry and return the **surviving** tool specs.
    #[must_use]
    pub fn allowed_tools(&self, registry: &ToolRegistry) -> Vec<Arc<dyn ToolSpec>> {
        registry
            .all()
            .into_iter()
            .filter(|t| self.is_allowed(t.as_ref()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::spec::{ApprovalRequirement, ToolContext, ToolError, ToolResult};
    use async_trait::async_trait;
    use serde_json::Value;

    /// A hand-rolled `ToolSpec` used to exercise the capability policy.
    struct TestTool {
        name: String,
        caps: Vec<ToolCapability>,
    }

    #[async_trait]
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
            ApprovalRequirement::Auto
        }
        async fn execute(
            &self,
            _input: Value,
            _ctx: &ToolContext,
        ) -> Result<ToolResult, ToolError> {
            Ok(ToolResult::success("ok".to_string()))
        }
    }

    fn ro(name: &str) -> Arc<dyn ToolSpec> {
        Arc::new(TestTool {
            name: name.to_string(),
            caps: vec![ToolCapability::ReadOnly],
        })
    }
    fn net(name: &str) -> Arc<dyn ToolSpec> {
        Arc::new(TestTool {
            name: name.to_string(),
            caps: vec![ToolCapability::Network],
        })
    }
    fn ro_net(name: &str) -> Arc<dyn ToolSpec> {
        Arc::new(TestTool {
            name: name.to_string(),
            caps: vec![ToolCapability::ReadOnly, ToolCapability::Network],
        })
    }
    fn write(name: &str) -> Arc<dyn ToolSpec> {
        Arc::new(TestTool {
            name: name.to_string(),
            caps: vec![ToolCapability::WritesFiles],
        })
    }

    fn test_registry() -> ToolRegistry {
        let mut reg = ToolRegistry::new(ToolContext::new(
            std::env::temp_dir().join("mimofan_cap_policy_test_ws"),
        ));
        reg.register_all(vec![
            ro("read_file"),
            net("web_search"),
            ro_net("proxy"),
            write("write_file"),
        ]);
        reg
    }

    #[test]
    fn deny_network_blocks_network_tools_but_allows_read_only() {
        let mut policy = CapabilityPermissionPolicy::new();
        policy.deny_capability(ToolCapability::Network);

        // A purely read-only tool is permitted.
        assert!(
            policy.is_allowed(ro("read_file").as_ref()),
            "ReadOnly tool must be allowed"
        );
        // A network-only tool is refused.
        assert!(
            !policy.is_allowed(net("web_search").as_ref()),
            "Network tool must be denied"
        );
        // A tool carrying BOTH ReadOnly and Network is refused because the
        // deny is capability-granular: any denied capability kills the tool.
        assert!(
            !policy.is_allowed(ro_net("proxy").as_ref()),
            "tool carrying a denied capability must be refused even if read-only"
        );
    }

    #[test]
    fn allow_all_permits_everything() {
        let policy = CapabilityPermissionPolicy::allow_all();
        assert!(policy.is_allowed(ro("read_file").as_ref()));
        assert!(policy.is_allowed(net("web_search").as_ref()));
        assert!(policy.is_allowed(write("write_file").as_ref()));
        assert!(policy.is_allowed(ro_net("proxy").as_ref()));
    }

    #[test]
    fn check_before_execute_returns_permission_denied() {
        let mut policy = CapabilityPermissionPolicy::new();
        policy.deny_capability(ToolCapability::Network);

        assert!(
            policy
                .check_before_execute(ro("read_file").as_ref())
                .is_ok(),
            "read-only tool should pass the gate"
        );
        let err = policy
            .check_before_execute(net("web_search").as_ref())
            .expect_err("network tool must be refused by the gate");
        match err {
            ToolError::PermissionDenied { message } => {
                assert!(
                    message.contains("Network"),
                    "denied capability should be named in the error: {message}"
                );
            }
            other => panic!("expected PermissionDenied, got {other:?}"),
        }
    }

    #[test]
    fn registry_filter_returns_expected_surviving_set() {
        let mut policy = CapabilityPermissionPolicy::new();
        policy.deny_capability(ToolCapability::Network);

        let reg = test_registry();
        let mut allowed = policy.allowed_tool_names(&reg);
        allowed.sort();

        // Surviving set = everything except the two network-capable tools.
        assert_eq!(
            allowed,
            vec!["read_file".to_string(), "write_file".to_string()]
        );
    }

    #[test]
    fn registry_filter_with_allow_all_keeps_everything() {
        let policy = CapabilityPermissionPolicy::allow_all();
        let reg = test_registry();
        let mut allowed = policy.allowed_tool_names(&reg);
        allowed.sort();
        assert_eq!(
            allowed,
            vec![
                "proxy".to_string(),
                "read_file".to_string(),
                "web_search".to_string(),
                "write_file".to_string()
            ]
        );
    }

    #[test]
    fn deny_multiple_capabilities_accumulate() {
        let mut policy = CapabilityPermissionPolicy::new();
        policy
            .deny_capability(ToolCapability::Network)
            .deny_capability(ToolCapability::WritesFiles);

        assert!(policy.is_allowed(ro("read_file").as_ref()));
        assert!(!policy.is_allowed(net("web_search").as_ref()));
        assert!(!policy.is_allowed(write("write_file").as_ref()));
        assert!(!policy.is_allowed(ro_net("proxy").as_ref()));
    }
}
