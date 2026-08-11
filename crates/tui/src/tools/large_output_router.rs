//! Large-output routing for tool results (issue #548).
//!
//! Any tool result whose estimated token count exceeds the configured threshold
//! is intercepted here before it reaches the parent context. A lightweight
//! V4-Flash synthesis sub-agent condenses the raw output; only the synthesis
//! is returned to the parent. The raw content is stored in the workshop
//! variable `last_tool_result` so the parent agent can call
//! `promote_to_context` later if it needs the full text.
//!
//! Per-tool thresholds can override the global default. Individual tool calls
//! may pass `raw=true` to bypass routing entirely.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::tokenizer::count_tokens;
use crate::tools::spec::ToolResult;

// ── Constants ──────────────────────────────────────────────────────────────────

/// Default token threshold above which a tool result is routed through the
/// workshop. Matches the issue spec of 4 096 tokens.
pub const DEFAULT_LARGE_OUTPUT_THRESHOLD_TOKENS: usize = 4_096;

/// Workshop variable name where the raw tool output is stored.
pub const WORKSHOP_LAST_TOOL_RESULT_VAR: &str = "last_tool_result";

// ── Configuration ─────────────────────────────────────────────────────────────

/// `[workshop]` section in `config.toml`.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct WorkshopConfig {
    /// Token threshold above which tool results are routed through the workshop
    /// synthesis sub-agent. Default: [`DEFAULT_LARGE_OUTPUT_THRESHOLD_TOKENS`].
    #[serde(default)]
    pub large_output_threshold_tokens: Option<usize>,

    /// Per-tool threshold overrides (tool name → token limit). A tool whose
    /// name appears here uses this limit instead of
    /// `large_output_threshold_tokens`.
    #[serde(default)]
    pub per_tool_thresholds: Option<HashMap<String, usize>>,
}

impl WorkshopConfig {
    /// Resolve the effective threshold for the given tool name.
    #[must_use]
    pub fn threshold_for(&self, tool_name: &str) -> usize {
        if let Some(per_tool) = self.per_tool_thresholds.as_ref()
            && let Some(&limit) = per_tool.get(tool_name)
        {
            return limit;
        }
        self.large_output_threshold_tokens
            .unwrap_or(DEFAULT_LARGE_OUTPUT_THRESHOLD_TOKENS)
    }
}

// ── Token estimation ──────────────────────────────────────────────────────────

/// Count the number of tokens in `text` using the real BPE tokenizer.
///
/// Previously this used a `chars / 3` heuristic to avoid a tokeniser
/// dependency. Now that [`crate::tokenizer`] is the single authoritative
/// counter, routing decisions use exact counts — which matters most for CJK
/// tool output, where the old heuristic was far off.
#[must_use]
pub fn estimate_tokens(text: &str) -> usize {
    count_tokens(text)
}

// ── Router ────────────────────────────────────────────────────────────────────

/// Decision returned by [`LargeOutputRouter::route`].
#[derive(Debug, Clone, PartialEq)]
pub enum RouteDecision {
    /// The output is small enough; pass it through unmodified.
    PassThrough,
    /// The output exceeded the threshold and was (or should be) synthesised.
    Synthesise {
        /// Estimated token count of the raw output.
        estimated_tokens: usize,
        /// The threshold that was breached.
        threshold: usize,
    },
}

/// Intercepts tool results and routes large ones through the workshop.
///
/// This type is intentionally `Clone` and `Default` so it can be embedded
/// cheaply in [`ToolContext`](crate::tools::spec::ToolContext) without
/// requiring `Arc` wrappers.
#[derive(Debug, Clone, Default)]
pub struct LargeOutputRouter {
    config: WorkshopConfig,
}

impl LargeOutputRouter {
    /// Construct a router from the resolved workshop config.
    #[must_use]
    pub fn new(config: WorkshopConfig) -> Self {
        Self { config }
    }

    /// Decide whether `result` for `tool_name` should be synthesised.
    ///
    /// Pass `raw_bypass = true` when the tool call included `raw = true`.
    #[must_use]
    pub fn route(&self, tool_name: &str, result: &ToolResult, raw_bypass: bool) -> RouteDecision {
        if raw_bypass || !result.success {
            return RouteDecision::PassThrough;
        }
        let threshold = self.config.threshold_for(tool_name);
        let estimated_tokens = estimate_tokens(&result.content);
        if estimated_tokens > threshold {
            RouteDecision::Synthesise {
                estimated_tokens,
                threshold,
            }
        } else {
            RouteDecision::PassThrough
        }
    }

    /// Wrap a synthesis result with a workshop provenance header and a hint
    /// about the stored raw output.
    #[must_use]
    pub fn wrap_synthesis(
        tool_name: &str,
        synthesis: &str,
        estimated_tokens: usize,
        threshold: usize,
    ) -> String {
        format!(
            "[workshop-synthesis: tool={tool_name}, raw_tokens≈{estimated_tokens}, \
             threshold={threshold}, raw_stored_in={WORKSHOP_LAST_TOOL_RESULT_VAR}]\n\n{synthesis}"
        )
    }
}

// ── Workshop variable store ───────────────────────────────────────────────────

/// In-process store for workshop variables that persist across tool calls
/// within a session. The only variable exposed today is `last_tool_result`
/// which holds the most recent raw large-tool output for `promote_to_context`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkshopVariables {
    /// Raw content of the most recent large tool output that was routed
    /// through the workshop. Empty string when no routing has occurred.
    #[serde(default)]
    pub last_tool_result: String,

    /// Name of the tool that produced `last_tool_result`.
    #[serde(default)]
    pub last_tool_name: String,
}

impl WorkshopVariables {
    /// Store the raw output from a large-tool routing event.
    pub fn store_raw(&mut self, tool_name: &str, raw: &str) {
        self.last_tool_result = raw.to_string();
        self.last_tool_name = tool_name.to_string();
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::spec::ToolResult;

    fn ok_result(content: &str) -> ToolResult {
        ToolResult::success(content)
    }

    fn fail_result(content: &str) -> ToolResult {
        let mut r = ToolResult::success(content);
        r.success = false;
        r
    }

    // ── threshold_for ──────────────────────────────────────────────────────

    #[test]
    fn threshold_defaults_to_constant() {
        let cfg = WorkshopConfig::default();
        assert_eq!(cfg.threshold_for("any_tool"), DEFAULT_LARGE_OUTPUT_THRESHOLD_TOKENS);
    }

    #[test]
    fn threshold_uses_global_override() {
        let cfg = WorkshopConfig {
            large_output_threshold_tokens: Some(100),
            per_tool_thresholds: None,
        };
        assert_eq!(cfg.threshold_for("any_tool"), 100);
    }

    #[test]
    fn per_tool_override_beats_global() {
        let mut per_tool = HashMap::new();
        per_tool.insert("grep".to_string(), 50);
        let cfg = WorkshopConfig {
            large_output_threshold_tokens: Some(100),
            per_tool_thresholds: Some(per_tool),
        };
        assert_eq!(cfg.threshold_for("grep"), 50);
        // Tools not in the map fall back to the global override.
        assert_eq!(cfg.threshold_for("read"), 100);
    }

    // ── route ──────────────────────────────────────────────────────────────

    #[test]
    fn route_passes_small_output_through() {
        let router = LargeOutputRouter::default();
        // Short ASCII text is well under the 4 096-token default threshold.
        let decision = router.route("read", &ok_result("hello world"), false);
        assert_eq!(decision, RouteDecision::PassThrough);
    }

    #[test]
    fn route_synthesises_large_output() {
        let router = LargeOutputRouter::default();
        // 2 000 repetitions of an 8-char line comfortably exceeds 4 096 tokens.
        let big = "line data\n".repeat(2_000);
        let decision = router.route("read", &ok_result(&big), false);
        match decision {
            RouteDecision::Synthesise {
                estimated_tokens,
                threshold,
            } => {
                assert!(estimated_tokens > threshold);
                assert_eq!(threshold, DEFAULT_LARGE_OUTPUT_THRESHOLD_TOKENS);
            }
            RouteDecision::PassThrough => panic!("expected Synthesise for large output"),
        }
    }

    #[test]
    fn route_bypasses_on_raw_flag() {
        let router = LargeOutputRouter::default();
        let big = "line data\n".repeat(2_000);
        // Even when over threshold, raw=true must pass through unchanged.
        let decision = router.route("read", &ok_result(&big), true);
        assert_eq!(decision, RouteDecision::PassThrough);
    }

    #[test]
    fn route_bypasses_on_failure() {
        let router = LargeOutputRouter::default();
        let big = "line data\n".repeat(2_000);
        // Failed tool results are never synthesised.
        let decision = router.route("read", &fail_result(&big), false);
        assert_eq!(decision, RouteDecision::PassThrough);
    }

    #[test]
    fn route_honors_per_tool_threshold() {
        let mut per_tool = HashMap::new();
        per_tool.insert("read".to_string(), 100); // per-tool limit of 100 tokens
        let cfg = WorkshopConfig {
            large_output_threshold_tokens: None,
            per_tool_thresholds: Some(per_tool),
        };
        let router = LargeOutputRouter::new(cfg);
        // A clearly over-100-token body must synthesise under the per-tool limit.
        let big = "token data line\n".repeat(200);
        let decision = router.route("read", &ok_result(&big), false);
        assert!(matches!(decision, RouteDecision::Synthesise { .. }));
    }

    // ── wrap_synthesis ─────────────────────────────────────────────────────

    #[test]
    fn wrap_synthesis_includes_provenance() {
        let wrapped = LargeOutputRouter::wrap_synthesis("grep", "summary text", 1234, 4096);
        assert!(wrapped.contains("tool=grep"));
        assert!(wrapped.contains("raw_tokens≈1234"));
        assert!(wrapped.contains("threshold=4096"));
        assert!(wrapped.contains(WORKSHOP_LAST_TOOL_RESULT_VAR));
        assert!(wrapped.contains("summary text"));
    }

    // ── WorkshopVariables ──────────────────────────────────────────────────

    #[test]
    fn store_raw_records_tool_and_content() {
        let mut vars = WorkshopVariables::default();
        assert!(vars.last_tool_result.is_empty());
        vars.store_raw("read", "big output body");
        assert_eq!(vars.last_tool_result, "big output body");
        assert_eq!(vars.last_tool_name, "read");
    }

    #[test]
    fn store_raw_overwrites_previous() {
        let mut vars = WorkshopVariables::default();
        vars.store_raw("grep", "first");
        vars.store_raw("read", "second");
        assert_eq!(vars.last_tool_result, "second");
        assert_eq!(vars.last_tool_name, "read");
    }
}
