// ── Mode & approval prompts as request-time runtime metadata ─────────
//
// Mode contracts and approval policies are not persisted in the session
// history and are not sent as extra system messages. Instead, each API
// request projects a transient user-role runtime metadata message at the
// tail. The stable system prompt remains byte-stable, stored history remains
// byte-stable, and strict chat-template providers never see a system message
// outside messages[0].

use std::path::Path;

use mimofan_execpolicy::{AskForApproval, ExecPolicyContext};
use serde_json::Value;

use super::EngineConfig;
use crate::core::ops::UserInputProvenance;
use crate::tui::app::AppMode;
use crate::tui::auto_review::{self, AutoReviewPolicy};

use super::tool_catalog::REQUEST_USER_INPUT_NAME;

#[derive(Debug, Clone)]
pub(super) struct EffectiveInputPolicy {
    pub(super) mode: AppMode,
    pub(super) allow_shell: bool,
    pub(super) trust_mode: bool,
    pub(super) auto_approve: bool,
    pub(super) approval_mode: crate::tui::approval::ApprovalMode,
    pub(super) dynamic_active_tools: Vec<&'static str>,
    pub(super) status: Option<String>,
}

pub(super) fn effective_input_policy(
    provenance: UserInputProvenance,
    requested_mode: AppMode,
    content: &str,
    allow_shell: bool,
    trust_mode: bool,
    auto_approve: bool,
    approval_mode: crate::tui::approval::ApprovalMode,
) -> EffectiveInputPolicy {
    let mut mode = requested_mode;
    let mut trust_mode = trust_mode;
    let mut auto_approve = auto_approve;
    let mut approval_mode = approval_mode;
    let mut dynamic_active_tools = Vec::new();
    let mut status = None;

    if !provenance.can_authorize_work() {
        let had_auto_authority = matches!(mode, AppMode::Yolo)
            || trust_mode
            || auto_approve
            || matches!(approval_mode, crate::tui::approval::ApprovalMode::Auto);
        if matches!(mode, AppMode::Yolo) {
            mode = AppMode::Agent;
        }
        trust_mode = false;
        auto_approve = false;
        if matches!(approval_mode, crate::tui::approval::ApprovalMode::Auto) {
            approval_mode = crate::tui::approval::ApprovalMode::Suggest;
        }
        if had_auto_authority {
            status = Some(format!(
                "Input provenance '{}' is not external user input; continuing with approvals required.",
                provenance.as_str()
            ));
        }
    } else if is_review_only_user_intent(content) {
        // Advisory only: never silently override an explicitly chosen mode
        // or strip its tools. Surface the question modal dynamically so the
        // model can ask focused follow-ups without inflating every tool prompt.
        dynamic_active_tools.push(REQUEST_USER_INPUT_NAME);
        status = Some(
            "Review/inspection request detected; keeping the current mode and exposing request_user_input for focused follow-up questions.".to_string(),
        );
    }

    EffectiveInputPolicy {
        mode,
        allow_shell,
        trust_mode,
        auto_approve,
        approval_mode,
        dynamic_active_tools,
        status,
    }
}

fn is_review_only_user_intent(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    let asks_to_inspect = [
        "look",
        "check",
        "review",
        "inspect",
        "scan",
        "audit",
        "看看",
        "看一下",
        "检查",
        "审查",
    ]
    .iter()
    .any(|needle| lower.contains(needle));
    if !asks_to_inspect {
        return false;
    }

    let explicit_write = [
        "fix",
        "change",
        "update",
        "implement",
        "apply",
        "patch",
        "modify",
        "edit",
        "write",
        "commit",
        "修",
        "改",
        "补",
        "提交",
        "写",
    ]
    .iter()
    .any(|needle| lower.contains(needle));

    !explicit_write
}

pub(super) fn agent_approval_mode_for_turn(
    auto_approve: bool,
    approval_mode: crate::tui::approval::ApprovalMode,
) -> crate::tui::approval::ApprovalMode {
    if auto_approve {
        crate::tui::approval::ApprovalMode::Auto
    } else {
        approval_mode
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ToolAskRuleDecision {
    Prompt(String),
    Block(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum AutoReviewPlanDecision {
    NoChange,
    ForcePrompt(String),
    Block(String),
}

pub(super) fn auto_review_run_origin_for_plan(
    detached_start: bool,
) -> crate::tui::auto_review::RunOrigin {
    if detached_start {
        crate::tui::auto_review::RunOrigin::Background
    } else {
        crate::tui::auto_review::RunOrigin::Interactive
    }
}

// The parameter list intentionally mirrors `AutoReviewContext::from_tool_call`,
// which this thin wrapper builds; the 8 call sites (1 prod + tests) read clearer
// passing the fields than constructing a context first.
#[allow(clippy::too_many_arguments)]
pub(super) fn auto_review_plan_decision(
    policy: &AutoReviewPolicy,
    tool_name: &str,
    tool_input: &Value,
    run_origin: auto_review::RunOrigin,
    approval_mode: crate::tui::approval::ApprovalMode,
    user_intent: Option<&str>,
    workspace_trusted: bool,
    dirty_worktree: bool,
) -> (AutoReviewPlanDecision, Value) {
    let context = auto_review::AutoReviewContext::from_tool_call(
        tool_name,
        tool_input,
        run_origin,
        approval_mode,
        user_intent,
        workspace_trusted,
        dirty_worktree,
    );
    let decision = policy.evaluate(&context);
    let audit_event = policy.audit_event(&context, &decision);
    let plan_decision = match decision.action {
        auto_review::AutoReviewAction::Allow | auto_review::AutoReviewAction::AskUser => {
            AutoReviewPlanDecision::NoChange
        }
        auto_review::AutoReviewAction::HoldForReview => {
            let reason = format!("Auto-review policy requires approval: {}", decision.reason);
            if matches!(approval_mode, crate::tui::approval::ApprovalMode::Never) {
                AutoReviewPlanDecision::Block(reason)
            } else {
                AutoReviewPlanDecision::ForcePrompt(reason)
            }
        }
        auto_review::AutoReviewAction::Block => AutoReviewPlanDecision::Block(format!(
            "Auto-review policy blocked tool '{tool_name}': {}",
            decision.reason
        )),
    };
    (plan_decision, audit_event)
}

pub(super) fn exec_shell_ask_rule_decision(
    config: &EngineConfig,
    tool_name: &str,
    tool_input: &Value,
    workspace: &Path,
    approval_mode: crate::tui::approval::ApprovalMode,
) -> Option<ToolAskRuleDecision> {
    if tool_name != "exec_shell" {
        return None;
    }
    let command = tool_input.get("command").and_then(Value::as_str)?;
    tool_ask_rule_decision_for_context(config, tool_name, command, None, workspace, approval_mode)
}

pub(super) fn file_tool_ask_rule_decision(
    config: &EngineConfig,
    tool_name: &str,
    tool_input: &Value,
    workspace: &Path,
    approval_mode: crate::tui::approval::ApprovalMode,
) -> Option<ToolAskRuleDecision> {
    let paths = file_tool_permission_paths(tool_name, tool_input)?;
    if paths.is_empty() {
        return tool_ask_rule_decision_for_context(
            config,
            tool_name,
            "",
            None,
            workspace,
            approval_mode,
        );
    }

    let mut prompt: Option<String> = None;
    for path in paths {
        match tool_ask_rule_decision_for_context(
            config,
            tool_name,
            "",
            Some(&path),
            workspace,
            approval_mode,
        ) {
            Some(ToolAskRuleDecision::Block(reason)) => {
                return Some(ToolAskRuleDecision::Block(reason));
            }
            Some(ToolAskRuleDecision::Prompt(reason)) => {
                prompt.get_or_insert(reason);
            }
            None => {}
        }
    }
    prompt.map(ToolAskRuleDecision::Prompt)
}

fn tool_ask_rule_decision_for_context(
    config: &EngineConfig,
    tool_name: &str,
    command: &str,
    path: Option<&str>,
    workspace: &Path,
    approval_mode: crate::tui::approval::ApprovalMode,
) -> Option<ToolAskRuleDecision> {
    let cwd = workspace.to_string_lossy();
    let ask_for_approval = match approval_mode {
        crate::tui::approval::ApprovalMode::Never => AskForApproval::Never,
        crate::tui::approval::ApprovalMode::Auto | crate::tui::approval::ApprovalMode::Suggest => {
            AskForApproval::OnFailure
        }
    };
    let decision = config
        .exec_policy_engine
        .check(ExecPolicyContext {
            command,
            cwd: cwd.as_ref(),
            tool: Some(tool_name),
            path,
            ask_for_approval,
            sandbox_mode: None,
        })
        .ok()?;
    if !decision.allow {
        Some(ToolAskRuleDecision::Block(decision.reason().to_string()))
    } else if decision.requires_approval {
        Some(ToolAskRuleDecision::Prompt(decision.reason().to_string()))
    } else {
        None
    }
}

fn file_tool_permission_paths(tool_name: &str, input: &Value) -> Option<Vec<String>> {
    match tool_name {
        "read_file" | "write_file" | "edit_file" | "file_search" | "grep_files" => {
            Some(string_field(input, "path").into_iter().collect())
        }
        "list_dir" => Some(vec![
            string_field(input, "path").unwrap_or_else(|| ".".to_string()),
        ]),
        "apply_patch" => Some(apply_patch_permission_paths(input)),
        _ => None,
    }
}

fn string_field(input: &Value, key: &str) -> Option<String> {
    input
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn apply_patch_permission_paths(input: &Value) -> Vec<String> {
    crate::tools::apply_patch::preflight_apply_patch(input)
        .map(|preflight| preflight.touched_files)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ops::UserInputProvenance;
    use crate::tui::app::AppMode;
    use crate::tui::approval::ApprovalMode;

    /// #677: privilege-escalation interception baseline.
    ///
    /// Any provenance other than `ExternalUser` must NOT be allowed to
    /// authorise work. Concretely, the four auto-authority signals —
    /// Yolo mode, `trust_mode`, `auto_approve`, and `ApprovalMode::Auto` —
    /// must all be stripped when the input did not come from an external user.
    /// This is a determinism contract: the same (provenance, flags) always
    /// produces the same downgraded policy.

    fn baseline_for(provenance: UserInputProvenance, mode: AppMode, trust: bool, auto: bool, am: ApprovalMode) -> EffectiveInputPolicy {
        effective_input_policy(provenance, mode, "do something", true, trust, auto, am)
    }

    #[test]
    fn external_user_retains_authority() {
        let p = baseline_for(
            UserInputProvenance::ExternalUser,
            AppMode::Yolo,
            true,
            true,
            ApprovalMode::Auto,
        );
        assert_eq!(p.mode, AppMode::Yolo);
        assert!(p.trust_mode);
        assert!(p.auto_approve);
        assert_eq!(p.approval_mode, ApprovalMode::Auto);
    }

    #[test]
    fn non_external_strips_yolo_mode() {
        let p = baseline_for(
            UserInputProvenance::Runtime,
            AppMode::Yolo,
            false,
            false,
            ApprovalMode::Suggest,
        );
        assert_eq!(p.mode, AppMode::Agent, "Yolo must downgrade to Agent");
        assert!(!p.trust_mode);
        assert!(!p.auto_approve);
    }

    #[test]
    fn non_external_strips_trust_and_auto_approve() {
        let p = baseline_for(
            UserInputProvenance::SubAgentHandoff,
            AppMode::Agent,
            true,
            true,
            ApprovalMode::Suggest,
        );
        assert!(!p.trust_mode);
        assert!(!p.auto_approve);
    }

    #[test]
    fn non_external_downgrades_auto_approval_mode() {
        let p = baseline_for(
            UserInputProvenance::ImportedTranscript,
            AppMode::Agent,
            false,
            false,
            ApprovalMode::Auto,
        );
        assert_eq!(p.approval_mode, ApprovalMode::Suggest);
    }

    #[test]
    fn every_non_external_provenance_is_denied_authority() {
        // Exhaustively assert the invariant across all non-external variants.
        for provenance in [
            UserInputProvenance::Runtime,
            UserInputProvenance::SubAgentHandoff,
            UserInputProvenance::ImportedTranscript,
            UserInputProvenance::MemoryRecall,
            UserInputProvenance::AssistantGenerated,
        ] {
            let p = baseline_for(provenance, AppMode::Yolo, true, true, ApprovalMode::Auto);
            assert!(
                !matches!(p.mode, AppMode::Yolo)
                    && !p.trust_mode
                    && !p.auto_approve
                    && p.approval_mode != ApprovalMode::Auto,
                "provenance {provenance:?} must not retain auto-authority"
            );
        }
    }
}
