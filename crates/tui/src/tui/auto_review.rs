//! Deterministic auto-review policy evaluation for tool calls.
//!
//! This module is intentionally narrow: it classifies a proposed tool action
//! into a review outcome and emits enough structured context for audit logs.
//! Enforcement and pre-push receipts are wired by higher-level surfaces.

#![allow(dead_code)]

use crate::tui::approval::{
    ApprovalMode, RiskLevel, ToolCategory, classify_risk, get_tool_category,
};
use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoReviewAction {
    Allow,
    AskUser,
    HoldForReview,
    Block,
}

impl AutoReviewAction {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::AskUser => "ask_user",
            Self::HoldForReview => "hold_for_review",
            Self::Block => "block",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoReviewDecision {
    pub action: AutoReviewAction,
    pub reason: String,
    pub rule_id: Option<String>,
}

impl AutoReviewDecision {
    fn new(action: AutoReviewAction, reason: impl Into<String>) -> Self {
        Self {
            action,
            reason: reason.into(),
            rule_id: None,
        }
    }

    fn with_rule(mut self, rule_id: impl Into<String>) -> Self {
        self.rule_id = Some(rule_id.into());
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolActionKind {
    Read,
    Write,
    Shell,
    Network,
    Git,
    McpRead,
    McpAction,
    Browser,
    Secret,
    Publish,
    Destructive,
    Unknown,
}

impl ToolActionKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::Shell => "shell",
            Self::Network => "network",
            Self::Git => "git",
            Self::McpRead => "mcp_read",
            Self::McpAction => "mcp_action",
            Self::Browser => "browser",
            Self::Secret => "secret",
            Self::Publish => "publish",
            Self::Destructive => "destructive",
            Self::Unknown => "unknown",
        }
    }

    #[must_use]
    pub fn from_tool_name(tool_name: &str, category: ToolCategory) -> Self {
        Self::from_tool_call(tool_name, &Value::Null, category)
    }

    #[must_use]
    pub fn from_tool_call(tool_name: &str, params: &Value, category: ToolCategory) -> Self {
        let normalized = tool_name.to_ascii_lowercase();

        if contains_any(&normalized, &["push", "publish", "release", "tag"]) {
            return Self::Publish;
        }
        if contains_any(&normalized, &["secret", "token", "credential", "password"]) {
            return Self::Secret;
        }
        if contains_any(
            &normalized,
            &["delete", "destroy", "remove", "drop", "reset"],
        ) {
            return Self::Destructive;
        }
        if contains_any(&normalized, &["git_"]) {
            return Self::Git;
        }
        if contains_any(&normalized, &["browser", "chrome", "playwright"]) {
            return Self::Browser;
        }

        if matches!(category, ToolCategory::Shell) && shell_params_are_publish_like(params) {
            return Self::Publish;
        }

        match category {
            ToolCategory::Safe => Self::Read,
            ToolCategory::FileWrite => Self::Write,
            ToolCategory::Shell => Self::Shell,
            ToolCategory::Network => Self::Network,
            ToolCategory::McpRead => Self::McpRead,
            ToolCategory::McpAction => Self::McpAction,
            ToolCategory::Unknown => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunOrigin {
    Interactive,
    Headless,
    Background,
}

impl RunOrigin {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Interactive => "interactive",
            Self::Headless => "headless",
            Self::Background => "background",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoReviewContext<'a> {
    pub tool_name: &'a str,
    pub category: ToolCategory,
    pub risk: RiskLevel,
    pub action_kind: ToolActionKind,
    pub run_origin: RunOrigin,
    pub approval_mode: ApprovalMode,
    pub user_intent: Option<&'a str>,
    pub workspace_trusted: bool,
    pub dirty_worktree: bool,
}

impl<'a> AutoReviewContext<'a> {
    #[must_use]
    pub fn from_tool_call(
        tool_name: &'a str,
        params: &Value,
        run_origin: RunOrigin,
        approval_mode: ApprovalMode,
        user_intent: Option<&'a str>,
        workspace_trusted: bool,
        dirty_worktree: bool,
    ) -> Self {
        let category = get_tool_category(tool_name);
        let risk = classify_risk(tool_name, category, params);
        let action_kind = ToolActionKind::from_tool_call(tool_name, params, category);
        Self {
            tool_name,
            category,
            risk,
            action_kind,
            run_origin,
            approval_mode,
            user_intent,
            workspace_trusted,
            dirty_worktree,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoReviewRule {
    pub id: String,
    pub action: AutoReviewAction,
    pub tool_name: Option<String>,
    pub action_kind: Option<ToolActionKind>,
    pub text_contains: Option<String>,
    pub reason: String,
}

impl AutoReviewRule {
    #[must_use]
    pub fn block(id: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            action: AutoReviewAction::Block,
            tool_name: None,
            action_kind: None,
            text_contains: None,
            reason: reason.into(),
        }
    }

    #[must_use]
    pub fn allow(id: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            action: AutoReviewAction::Allow,
            tool_name: None,
            action_kind: None,
            text_contains: None,
            reason: reason.into(),
        }
    }

    #[must_use]
    pub fn tool_name(mut self, tool_name: impl Into<String>) -> Self {
        self.tool_name = Some(tool_name.into());
        self
    }

    #[must_use]
    pub fn action_kind(mut self, action_kind: ToolActionKind) -> Self {
        self.action_kind = Some(action_kind);
        self
    }

    #[must_use]
    pub fn text_contains(mut self, text: impl Into<String>) -> Self {
        self.text_contains = Some(text.into());
        self
    }

    fn matches(&self, ctx: &AutoReviewContext<'_>) -> bool {
        if let Some(tool_name) = self.tool_name.as_deref() {
            if tool_name != ctx.tool_name {
                return false;
            }
        }

        if let Some(action_kind) = self.action_kind {
            if action_kind != ctx.action_kind {
                return false;
            }
        }

        if let Some(text) = self.text_contains.as_deref() {
            let Some(user_intent) = ctx.user_intent else {
                return false;
            };
            if !user_intent
                .to_ascii_lowercase()
                .contains(&text.to_ascii_lowercase())
            {
                return false;
            }
        }

        true
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AutoReviewPolicy {
    pub allow_rules: Vec<AutoReviewRule>,
    pub block_rules: Vec<AutoReviewRule>,
    pub natural_language_guidance: Option<String>,
}

impl AutoReviewPolicy {
    #[must_use]
    pub fn evaluate(&self, ctx: &AutoReviewContext<'_>) -> AutoReviewDecision {
        if let Some(rule) = self
            .block_rules
            .iter()
            .find(|rule| rule.matches(ctx) && rule.action == AutoReviewAction::Block)
        {
            return AutoReviewDecision::new(AutoReviewAction::Block, rule.reason.clone())
                .with_rule(rule.id.clone());
        }

        if let Some(floor) = safety_floor(ctx) {
            return floor;
        }

        if let Some(rule) = self
            .allow_rules
            .iter()
            .find(|rule| rule.matches(ctx) && rule.action == AutoReviewAction::Allow)
        {
            return AutoReviewDecision::new(AutoReviewAction::Allow, rule.reason.clone())
                .with_rule(rule.id.clone());
        }

        deterministic_fallback(ctx)
    }

    #[must_use]
    pub fn audit_event(&self, ctx: &AutoReviewContext<'_>, decision: &AutoReviewDecision) -> Value {
        json!({
            "tool_name": ctx.tool_name,
            "tool_category": tool_category_label(ctx.category),
            "risk": risk_label(ctx.risk),
            "action_kind": ctx.action_kind.as_str(),
            "run_origin": ctx.run_origin.as_str(),
            "approval_mode": ctx.approval_mode.label(),
            "workspace_trusted": ctx.workspace_trusted,
            "dirty_worktree": ctx.dirty_worktree,
            "policy_has_guidance": self.natural_language_guidance.is_some(),
            "decision": decision.action.as_str(),
            "reason": decision.reason,
            "rule_id": decision.rule_id.as_deref(),
        })
    }
}

fn safety_floor(ctx: &AutoReviewContext<'_>) -> Option<AutoReviewDecision> {
    if matches!(ctx.action_kind, ToolActionKind::Publish) {
        return Some(AutoReviewDecision::new(
            AutoReviewAction::HoldForReview,
            "publish-like actions require a durable review step",
        ));
    }

    if matches!(ctx.run_origin, RunOrigin::Headless | RunOrigin::Background)
        && matches!(ctx.risk, RiskLevel::Destructive)
    {
        return Some(AutoReviewDecision::new(
            AutoReviewAction::HoldForReview,
            "destructive background/headless actions cannot auto-approve",
        ));
    }

    if !ctx.workspace_trusted && matches!(ctx.risk, RiskLevel::Destructive) {
        return Some(AutoReviewDecision::new(
            AutoReviewAction::AskUser,
            "destructive action in an untrusted workspace requires user review",
        ));
    }

    None
}

fn deterministic_fallback(ctx: &AutoReviewContext<'_>) -> AutoReviewDecision {
    match (ctx.category, ctx.risk, ctx.action_kind) {
        (ToolCategory::Safe | ToolCategory::McpRead, RiskLevel::Benign, _) => {
            AutoReviewDecision::new(AutoReviewAction::Allow, "read-only action is allowed")
        }
        (_, _, ToolActionKind::McpAction) => AutoReviewDecision::new(
            AutoReviewAction::HoldForReview,
            "MCP actions may have remote side effects",
        ),
        (ToolCategory::Unknown, _, _) => AutoReviewDecision::new(
            AutoReviewAction::AskUser,
            "unknown tool category requires explicit review",
        ),
        (_, RiskLevel::Destructive, _) => AutoReviewDecision::new(
            AutoReviewAction::AskUser,
            "destructive action requires explicit review",
        ),
        _ => AutoReviewDecision::new(
            AutoReviewAction::AskUser,
            "no deterministic allow rule matched",
        ),
    }
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

fn shell_params_are_publish_like(params: &Value) -> bool {
    let Some(command) = params
        .get("command")
        .or_else(|| params.get("cmd"))
        .and_then(Value::as_str)
    else {
        return false;
    };

    split_shell_segments_for_review(command)
        .iter()
        .map(|segment| {
            segment
                .split_whitespace()
                .filter(|token| !token.trim().is_empty())
                .collect::<Vec<_>>()
        })
        .any(|tokens| shell_tokens_are_publish_like(&tokens))
}

fn shell_tokens_are_publish_like(tokens: &[&str]) -> bool {
    if git_tag_tokens_are_publish_like(tokens) {
        return true;
    }

    let canonical = crate::command_safety::classify_command(tokens);
    matches!(
        canonical.as_str(),
        "git push" | "gh release" | "npm publish" | "cargo publish"
    )
}

fn git_tag_tokens_are_publish_like(tokens: &[&str]) -> bool {
    let Some(tag_index) = git_subcommand_index(tokens).filter(|index| {
        tokens
            .get(*index)
            .is_some_and(|token| shell_token_eq(token, "tag"))
    }) else {
        return false;
    };

    let mut list_like = false;
    let mut verify_only = false;
    let mut has_positional = false;
    let mut index = tag_index + 1;

    while let Some(token) = tokens.get(index).map(|token| shell_token_trim(token)) {
        match token {
            "-d" | "--delete" => return true,
            "-a" | "--annotate" | "-s" | "--sign" | "-f" | "--force" => {
                return true;
            }
            "-u" | "--local-user" | "-m" | "--message" | "-F" | "--file" => {
                return true;
            }
            "--list" | "-l" => list_like = true,
            "-n" | "--verify" | "-v" => verify_only = true,
            "--contains" | "--points-at" | "--merged" | "--no-merged" | "--sort" | "--format"
            | "--column" => {
                list_like = true;
                index += 1;
            }
            _ if token.starts_with("--list=")
                || token.starts_with("-n")
                || token.starts_with("--contains=")
                || token.starts_with("--points-at=")
                || token.starts_with("--merged=")
                || token.starts_with("--no-merged=")
                || token.starts_with("--sort=")
                || token.starts_with("--format=")
                || token.starts_with("--column=") =>
            {
                list_like = true;
            }
            _ if token.starts_with('-') => {}
            _ => has_positional = true,
        }

        index += 1;
    }

    has_positional && !list_like && !verify_only
}

fn git_subcommand_index(tokens: &[&str]) -> Option<usize> {
    if !tokens
        .first()
        .is_some_and(|token| shell_token_eq(token, "git"))
    {
        return None;
    }

    let mut index = 1;
    while let Some(token) = tokens.get(index).map(|token| shell_token_trim(token)) {
        if git_global_option_takes_value(token) {
            index += 2;
            continue;
        }

        if git_global_option_has_value(token) || token.starts_with('-') {
            index += 1;
            continue;
        }

        return Some(index);
    }

    None
}

fn git_global_option_takes_value(token: &str) -> bool {
    matches!(
        token,
        "-C" | "-c" | "--git-dir" | "--work-tree" | "--namespace" | "--config-env" | "--exec-path"
    )
}

fn git_global_option_has_value(token: &str) -> bool {
    token.starts_with("--git-dir=")
        || token.starts_with("--work-tree=")
        || token.starts_with("--namespace=")
        || token.starts_with("--config-env=")
        || token.starts_with("--exec-path=")
}

fn shell_token_eq(token: &str, expected: &str) -> bool {
    shell_token_trim(token).eq_ignore_ascii_case(expected)
}

fn shell_token_trim(token: &str) -> &str {
    token.trim_matches(|ch| matches!(ch, '\'' | '"'))
}

fn split_shell_segments_for_review(command: &str) -> Vec<String> {
    command
        .replace("&&", "\n")
        .replace("||", "\n")
        .replace(';', "\n")
        .lines()
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn tool_category_label(category: ToolCategory) -> &'static str {
    match category {
        ToolCategory::Safe => "safe",
        ToolCategory::FileWrite => "file_write",
        ToolCategory::Shell => "shell",
        ToolCategory::Network => "network",
        ToolCategory::McpRead => "mcp_read",
        ToolCategory::McpAction => "mcp_action",
        ToolCategory::Unknown => "unknown",
    }
}

fn risk_label(risk: RiskLevel) -> &'static str {
    match risk {
        RiskLevel::Benign => "benign",
        RiskLevel::Destructive => "destructive",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ctx_for(
        tool_name: &str,
        params: Value,
        run_origin: RunOrigin,
        approval_mode: ApprovalMode,
    ) -> AutoReviewContext<'_> {
        AutoReviewContext::from_tool_call(
            tool_name,
            &params,
            run_origin,
            approval_mode,
            Some("inspect the project status"),
            true,
            false,
        )
    }

    #[test]
    fn read_only_inspection_allows_by_default() {
        let policy = AutoReviewPolicy::default();
        let ctx = ctx_for(
            "read_file",
            json!({ "path": "README.md" }),
            RunOrigin::Interactive,
            ApprovalMode::Suggest,
        );

        let decision = policy.evaluate(&ctx);

        assert_eq!(decision.action, AutoReviewAction::Allow);
        assert!(decision.reason.contains("read-only"));
    }

    #[test]
    fn explicit_block_rule_blocks_destructive_shell() {
        let policy = AutoReviewPolicy {
            block_rules: vec![
                AutoReviewRule::block("no-rm", "rm commands are blocked")
                    .tool_name("exec_shell")
                    .text_contains("remove"),
            ],
            ..AutoReviewPolicy::default()
        };
        let ctx = AutoReviewContext::from_tool_call(
            "exec_shell",
            &json!({ "command": "rm -rf target" }),
            RunOrigin::Interactive,
            ApprovalMode::Auto,
            Some("remove generated build artifacts"),
            true,
            false,
        );

        let decision = policy.evaluate(&ctx);

        assert_eq!(decision.action, AutoReviewAction::Block);
        assert_eq!(decision.rule_id.as_deref(), Some("no-rm"));
    }

    #[test]
    fn headless_destructive_tool_holds_for_review_even_with_allow_rule() {
        let policy = AutoReviewPolicy {
            allow_rules: vec![
                AutoReviewRule::allow("allow-shell", "trusted shell command")
                    .action_kind(ToolActionKind::Shell),
            ],
            ..AutoReviewPolicy::default()
        };
        let ctx = ctx_for(
            "exec_shell",
            json!({ "command": "cargo publish" }),
            RunOrigin::Headless,
            ApprovalMode::Auto,
        );

        let decision = policy.evaluate(&ctx);

        assert_eq!(decision.action, AutoReviewAction::HoldForReview);
        assert!(decision.rule_id.is_none());
    }

    #[test]
    fn mcp_read_allows_and_mcp_action_holds() {
        let policy = AutoReviewPolicy::default();
        let read_ctx = ctx_for(
            "read_mcp_resource",
            json!({ "uri": "repo://summary" }),
            RunOrigin::Interactive,
            ApprovalMode::Suggest,
        );
        let action_ctx = ctx_for(
            "mcp_github_merge_pull_request",
            json!({ "pull_number": 123 }),
            RunOrigin::Interactive,
            ApprovalMode::Suggest,
        );

        assert_eq!(policy.evaluate(&read_ctx).action, AutoReviewAction::Allow);
        assert_eq!(
            policy.evaluate(&action_ctx).action,
            AutoReviewAction::HoldForReview
        );
    }

    #[test]
    fn git_push_like_action_holds_for_review() {
        let policy = AutoReviewPolicy::default();
        let ctx = ctx_for(
            "git_push",
            json!({ "remote": "origin", "branch": "main" }),
            RunOrigin::Interactive,
            ApprovalMode::Auto,
        );

        let decision = policy.evaluate(&ctx);

        assert_eq!(decision.action, AutoReviewAction::HoldForReview);
        assert!(decision.reason.contains("publish-like"));
    }

    #[test]
    fn shell_git_push_holds_for_publish_review() {
        let policy = AutoReviewPolicy::default();
        let ctx = ctx_for(
            "exec_shell",
            json!({ "command": "git push origin main" }),
            RunOrigin::Interactive,
            ApprovalMode::Auto,
        );

        let decision = policy.evaluate(&ctx);

        assert_eq!(ctx.action_kind, ToolActionKind::Publish);
        assert_eq!(decision.action, AutoReviewAction::HoldForReview);
        assert!(decision.reason.contains("publish-like"));
    }

    #[test]
    fn shell_chained_publish_command_holds_for_review() {
        let policy = AutoReviewPolicy::default();
        let ctx = ctx_for(
            "exec_shell",
            json!({ "command": "cargo test && npm publish" }),
            RunOrigin::Interactive,
            ApprovalMode::Auto,
        );

        let decision = policy.evaluate(&ctx);

        assert_eq!(ctx.action_kind, ToolActionKind::Publish);
        assert_eq!(decision.action, AutoReviewAction::HoldForReview);
    }

    #[test]
    fn shell_git_status_does_not_match_publish_review() {
        let ctx = ctx_for(
            "exec_shell",
            json!({ "command": "git status --porcelain" }),
            RunOrigin::Interactive,
            ApprovalMode::Auto,
        );

        assert_eq!(ctx.action_kind, ToolActionKind::Shell);
    }

    #[test]
    fn shell_git_tag_list_does_not_match_publish_review() {
        let ctx = ctx_for(
            "exec_shell",
            json!({ "command": "git remote -v && git rev-parse --show-toplevel && git branch --show-current && git rev-parse HEAD && git tag --list 'v0.8.65'" }),
            RunOrigin::Interactive,
            ApprovalMode::Auto,
        );

        assert_eq!(ctx.action_kind, ToolActionKind::Shell);
    }

    #[test]
    fn shell_git_tag_creation_holds_for_publish_review() {
        let policy = AutoReviewPolicy::default();
        let ctx = ctx_for(
            "exec_shell",
            json!({ "command": "git tag v0.8.65" }),
            RunOrigin::Interactive,
            ApprovalMode::Auto,
        );

        let decision = policy.evaluate(&ctx);

        assert_eq!(ctx.action_kind, ToolActionKind::Publish);
        assert_eq!(decision.action, AutoReviewAction::HoldForReview);
    }

    #[test]
    fn shell_git_tag_delete_holds_for_publish_review() {
        let policy = AutoReviewPolicy::default();
        let ctx = ctx_for(
            "exec_shell",
            json!({ "command": "git tag --delete v0.8.65" }),
            RunOrigin::Interactive,
            ApprovalMode::Auto,
        );

        let decision = policy.evaluate(&ctx);

        assert_eq!(ctx.action_kind, ToolActionKind::Publish);
        assert_eq!(decision.action, AutoReviewAction::HoldForReview);
    }

    #[test]
    fn guidance_does_not_override_deterministic_fallback() {
        let policy = AutoReviewPolicy {
            natural_language_guidance: Some("Prefer fast background fixes.".to_string()),
            ..AutoReviewPolicy::default()
        };
        let ctx = ctx_for(
            "mystery_tool",
            json!({ "value": true }),
            RunOrigin::Interactive,
            ApprovalMode::Suggest,
        );

        let decision = policy.evaluate(&ctx);

        assert_eq!(decision.action, AutoReviewAction::AskUser);
        assert!(decision.reason.contains("unknown"));
    }

    #[test]
    fn audit_event_includes_context_and_reason() {
        let policy = AutoReviewPolicy {
            natural_language_guidance: Some("Hold risky tools.".to_string()),
            ..AutoReviewPolicy::default()
        };
        let ctx = AutoReviewContext::from_tool_call(
            "read_file",
            &json!({ "path": "Cargo.toml" }),
            RunOrigin::Background,
            ApprovalMode::Suggest,
            Some("read manifest"),
            true,
            true,
        );
        let decision = policy.evaluate(&ctx);

        let event = policy.audit_event(&ctx, &decision);

        assert_eq!(event["tool_name"], "read_file");
        assert_eq!(event["tool_category"], "safe");
        assert_eq!(event["run_origin"], "background");
        assert_eq!(event["decision"], "allow");
        assert_eq!(event["reason"], "read-only action is allowed");
        assert_eq!(event["policy_has_guidance"], true);
        assert_eq!(event["dirty_worktree"], true);
    }
}
