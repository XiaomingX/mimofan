use mimofan_execpolicy::*;
use mimofan_protocol::{NetworkPolicyAmendment, NetworkPolicyRuleAction};

fn ctx(command: &str, ask_for_approval: AskForApproval) -> ExecPolicyContext<'_> {
    ExecPolicyContext {
        command,
        cwd: "/workspace",
        tool: Some("exec_shell"),
        path: None,
        ask_for_approval,
        sandbox_mode: Some("workspace-write"),
    }
}

#[test]
fn deny_rule_blocks_path_and_wrapper_bypasses() {
    // Regression: a `deny` rule for `rm` must block absolute-path and
    // wrapper invocations, not just the bare command.
    let engine = ExecPolicyEngine::new(vec![], vec!["rm".to_string()]);

    for cmd in [
        "rm -rf /",
        "/bin/rm -rf /",
        "sudo rm -rf /",
        "command rm -rf /",
    ] {
        let decision = engine
            .check(ctx(cmd, AskForApproval::Never))
            .expect("deny_rule_blocks_path_and_wrapper_bypasses");
        assert!(!decision.allow, "expected block for {cmd}");
        assert!(
            matches!(
                decision.requirement,
                ExecApprovalRequirement::Forbidden { .. }
            ),
            "expected forbidden for {cmd}"
        );
    }

    // Non-matching commands must NOT be caught by the `rm` rule.
    for cmd in ["rmdir foo", "rmview", "git rm file"] {
        let decision = engine
            .check(ctx(cmd, AskForApproval::Never))
            .expect("deny_rule_blocks_path_and_wrapper_bypasses");
        assert!(decision.allow, "expected allow for {cmd}");
    }
}

#[test]
fn canonical_executable_form_strips_wrappers_and_path() {
    assert_eq!(canonical_executable_form("/bin/rm -rf /"), "rm -rf /");
    assert_eq!(canonical_executable_form("sudo rm -rf /"), "rm -rf /");
    assert_eq!(canonical_executable_form("command rm -rf /"), "rm -rf /");
    assert_eq!(
        canonical_executable_form("env FOO=bar rm -rf /"),
        "rm -rf /"
    );
    assert_eq!(canonical_executable_form("rmdir foo"), "rmdir foo");
}

#[test]
fn trusted_prefix_skips_approval_when_policy_is_unless_trusted() {
    let engine = ExecPolicyEngine::new(vec!["git status".to_string()], vec![]);

    let decision = engine
        .check(ctx("git status --porcelain", AskForApproval::UnlessTrusted))
        .expect("trusted_prefix_skips_approval_when_policy_is_unless_trusted");

    assert!(decision.allow);
    assert!(!decision.requires_approval);
    assert_eq!(decision.matched_rule.as_deref(), Some("git status"));
    assert!(matches!(
        decision.requirement,
        ExecApprovalRequirement::Skip {
            bypass_sandbox: false,
            proposed_execpolicy_amendment: None,
        }
    ));
}

#[test]
fn denied_prefix_blocks_even_when_command_is_also_trusted() {
    let engine = ExecPolicyEngine::new(
        vec!["git status".to_string()],
        vec!["git status".to_string()],
    );

    let decision = engine
        .check(ctx("git status --porcelain", AskForApproval::UnlessTrusted))
        .expect("denied_prefix_blocks_even_when_command_is_also_trusted");

    assert!(!decision.allow);
    assert!(!decision.requires_approval);
    assert_eq!(decision.matched_rule.as_deref(), Some("git status"));
    assert!(matches!(
        decision.requirement,
        ExecApprovalRequirement::Forbidden { .. }
    ));
    assert_eq!(
        decision.reason(),
        "Command blocked by denied prefix rule 'git status'"
    );
}

#[test]
fn unmatched_command_requires_approval_and_proposes_first_token_rule() {
    let engine = ExecPolicyEngine::new(vec![], vec![]);

    let decision = engine
        .check(ctx("cargo test --workspace", AskForApproval::UnlessTrusted))
        .expect("unmatched_command_requires_approval_and_proposes_first_token_rule");

    assert!(decision.allow);
    assert!(decision.requires_approval);
    assert_eq!(decision.matched_rule, None);
    match decision.requirement {
        ExecApprovalRequirement::NeedsApproval {
            proposed_execpolicy_amendment: Some(amendment),
            proposed_network_policy_amendments,
            ..
        } => {
            assert_eq!(amendment.prefixes, vec!["cargo"]);
            assert_eq!(
                proposed_network_policy_amendments,
                vec![NetworkPolicyAmendment {
                    host: "/workspace".to_string(),
                    action: NetworkPolicyRuleAction::Allow,
                }]
            );
        }
        other => panic!("expected approval with proposed amendment, got {other:?}"),
    }
}

#[test]
fn trusted_command_in_on_request_mode_still_requires_approval_without_new_rule() {
    let engine = ExecPolicyEngine::new(vec!["cargo test".to_string()], vec![]);

    let decision = engine
        .check(ctx("cargo test --workspace", AskForApproval::OnRequest))
        .expect("trusted_command_in_on_request_mode_still_requires_approval_without_new_rule");

    assert!(decision.allow);
    assert!(decision.requires_approval);
    assert_eq!(decision.matched_rule.as_deref(), Some("cargo test"));
    match decision.requirement {
        ExecApprovalRequirement::NeedsApproval {
            proposed_execpolicy_amendment,
            ..
        } => assert_eq!(proposed_execpolicy_amendment, None),
        other => panic!("expected approval without amendment, got {other:?}"),
    }
}

#[test]
fn reject_rules_mode_forbids_unmatched_command() {
    let engine = ExecPolicyEngine::new(vec![], vec![]);

    let decision = engine
        .check(ctx(
            "npm install",
            AskForApproval::Reject {
                sandbox_approval: false,
                rules: true,
                mcp_elicitations: false,
            },
        ))
        .expect("reject_rules_mode_forbids_unmatched_command");

    assert!(!decision.allow);
    assert!(!decision.requires_approval);
    assert_eq!(decision.matched_rule, None);
    assert_eq!(decision.requirement.phase(), "forbidden");
    assert_eq!(
        decision.reason(),
        "Policy is configured to reject rule-exceptions."
    );
}

#[test]
fn typed_ask_rule_forbids_matching_command_when_policy_is_never() {
    let engine = ExecPolicyEngine::with_rulesets(vec![
        Ruleset::user(vec![], vec![]).with_ask_rules(vec![ToolAskRule::exec_shell("cargo test")]),
    ]);

    let decision = engine
        .check(ctx("cargo test --workspace", AskForApproval::Never))
        .expect("typed_ask_rule_forbids_matching_command_when_policy_is_never");

    assert!(!decision.allow);
    assert!(!decision.requires_approval);
    assert_eq!(
        decision.matched_rule.as_deref(),
        Some("tool=exec_shell command=cargo test")
    );
    assert_eq!(decision.requirement.phase(), "forbidden");
    assert_eq!(
        decision.reason(),
        "Typed ask rule 'tool=exec_shell command=cargo test' requires approval, but approval policy is never."
    );
}

#[test]
fn typed_ask_rule_requires_approval_under_unless_trusted() {
    let engine = ExecPolicyEngine::with_rulesets(vec![
        Ruleset::user(vec![], vec![]).with_ask_rules(vec![ToolAskRule::exec_shell("cargo test")]),
    ]);

    let decision = engine
        .check(ctx("cargo test --workspace", AskForApproval::UnlessTrusted))
        .expect("typed_ask_rule_requires_approval_under_unless_trusted");

    assert!(decision.allow);
    assert!(decision.requires_approval);
    assert_eq!(
        decision.matched_rule.as_deref(),
        Some("tool=exec_shell command=cargo test")
    );
    match decision.requirement {
        ExecApprovalRequirement::NeedsApproval {
            proposed_execpolicy_amendment,
            proposed_network_policy_amendments,
            ..
        } => {
            assert_eq!(proposed_execpolicy_amendment, None);
            // A typed ask-rule approval must not allow-list the cwd (or
            // anything else) as a network host. See the NeedsApproval arm.
            assert!(
                proposed_network_policy_amendments.is_empty(),
                "ask-rule approval must not propose network amendments, got {proposed_network_policy_amendments:?}"
            );
        }
        other => panic!("expected typed ask approval, got {other:?}"),
    }
}

#[test]
fn typed_ask_rule_requires_approval_under_on_failure() {
    let engine = ExecPolicyEngine::with_rulesets(vec![
        Ruleset::user(vec![], vec![]).with_ask_rules(vec![ToolAskRule::exec_shell("cargo test")]),
    ]);

    let decision = engine
        .check(ctx("cargo test --workspace", AskForApproval::OnFailure))
        .expect("typed_ask_rule_requires_approval_under_on_failure");

    assert!(decision.allow);
    assert!(decision.requires_approval);
    assert_eq!(
        decision.reason(),
        "Typed ask rule 'tool=exec_shell command=cargo test' requires approval."
    );
}

#[test]
fn typed_ask_rule_overrides_trusted_but_not_deny() {
    let engine = ExecPolicyEngine::with_rulesets(vec![
        Ruleset::user(
            vec!["cargo test".to_string()],
            vec!["cargo test --danger".to_string()],
        )
        .with_ask_rules(vec![ToolAskRule::exec_shell("cargo test")]),
    ]);

    let trusted = engine
        .check(ctx("cargo test --workspace", AskForApproval::UnlessTrusted))
        .expect("typed_ask_rule_overrides_trusted_but_not_deny");
    assert!(trusted.allow);
    assert!(trusted.requires_approval);
    assert_eq!(
        trusted.matched_rule.as_deref(),
        Some("tool=exec_shell command=cargo test")
    );

    let denied = engine
        .check(ctx("cargo test --danger", AskForApproval::Never))
        .expect("typed_ask_rule_overrides_trusted_but_not_deny");
    assert!(!denied.allow);
    assert!(!denied.requires_approval);
    assert_eq!(denied.matched_rule.as_deref(), Some("cargo test --danger"));
    assert_eq!(
        denied.reason(),
        "Command blocked by denied prefix rule 'cargo test --danger'"
    );
}

#[test]
fn typed_ask_rule_prefers_higher_layer_before_specificity() {
    let engine = ExecPolicyEngine::with_rulesets(vec![
        Ruleset::agent(vec![], vec![])
            .with_ask_rules(vec![ToolAskRule::exec_shell("cargo test --workspace")]),
        Ruleset::user(vec![], vec![]).with_ask_rules(vec![ToolAskRule::exec_shell("cargo test")]),
    ]);

    let decision = engine
        .check(ctx(
            "cargo test --workspace --all-features",
            AskForApproval::UnlessTrusted,
        ))
        .expect("typed_ask_rule_prefers_higher_layer_before_specificity");

    assert!(decision.requires_approval);
    assert_eq!(
        decision.matched_rule.as_deref(),
        Some("tool=exec_shell command=cargo test")
    );
}

#[test]
fn reject_rules_mode_still_forbids_matching_ask_rule() {
    let engine = ExecPolicyEngine::with_rulesets(vec![
        Ruleset::user(vec![], vec![]).with_ask_rules(vec![ToolAskRule::exec_shell("cargo test")]),
    ]);

    let decision = engine
        .check(ctx(
            "cargo test --workspace",
            AskForApproval::Reject {
                sandbox_approval: false,
                rules: true,
                mcp_elicitations: false,
            },
        ))
        .expect("reject_rules_mode_still_forbids_matching_ask_rule");

    assert!(!decision.allow);
    assert!(!decision.requires_approval);
    assert_eq!(decision.matched_rule, None);
    assert_eq!(
        decision.reason(),
        "Policy is configured to reject rule-exceptions."
    );
}

#[test]
fn typed_ask_rule_label_wins_when_never_blocks_trusted_command() {
    let engine = ExecPolicyEngine::with_rulesets(vec![
        Ruleset::user(vec!["cargo test".to_string()], vec![])
            .with_ask_rules(vec![ToolAskRule::exec_shell("cargo test")]),
    ]);

    let decision = engine
        .check(ctx("cargo test --workspace", AskForApproval::Never))
        .expect("typed_ask_rule_label_wins_when_never_blocks_trusted_command");

    assert!(!decision.allow);
    assert_eq!(
        decision.matched_rule.as_deref(),
        Some("tool=exec_shell command=cargo test")
    );
    assert_eq!(
        decision.reason(),
        "Typed ask rule 'tool=exec_shell command=cargo test' requires approval, but approval policy is never."
    );
}

#[test]
fn typed_ask_path_matching_trims_spaces_before_workspace_normalization() {
    let engine =
        ExecPolicyEngine::with_rulesets(vec![Ruleset::user(vec![], vec![]).with_ask_rules(vec![
            ToolAskRule::file_path("edit_file", " /workspace/tmp/project/ "),
        ])]);

    let decision = engine
        .check(ExecPolicyContext {
            command: "",
            cwd: "/workspace",
            tool: Some("edit_file"),
            path: Some("tmp/project"),
            ask_for_approval: AskForApproval::Never,
            sandbox_mode: Some("workspace-write"),
        })
        .expect("typed_ask_path_matching_trims_spaces_before_workspace_normalization");

    assert!(!decision.allow);
    assert_eq!(
        decision.matched_rule.as_deref(),
        Some("tool=edit_file path= /workspace/tmp/project/ ")
    );
}

#[test]
fn typed_ask_path_matching_normalizes_relative_and_absolute_workspace_paths() {
    let relative_rule = ExecPolicyEngine::with_rulesets(vec![
        Ruleset::user(vec![], vec![])
            .with_ask_rules(vec![ToolAskRule::file_path("edit_file", "src/a.rs")]),
    ]);
    let absolute_path = relative_rule
        .check(ExecPolicyContext {
            command: "",
            cwd: "/workspace",
            tool: Some("edit_file"),
            path: Some("/workspace/src/a.rs"),
            ask_for_approval: AskForApproval::OnFailure,
            sandbox_mode: Some("workspace-write"),
        })
        .expect("typed_ask_path_matching_normalizes_relative_and_absolute_workspace_paths");
    assert!(absolute_path.requires_approval);

    let absolute_rule =
        ExecPolicyEngine::with_rulesets(vec![Ruleset::user(vec![], vec![]).with_ask_rules(vec![
            ToolAskRule::file_path("edit_file", "/workspace/src/a.rs"),
        ])]);
    let relative_path = absolute_rule
        .check(ExecPolicyContext {
            command: "",
            cwd: "/workspace",
            tool: Some("edit_file"),
            path: Some("src/a.rs"),
            ask_for_approval: AskForApproval::OnFailure,
            sandbox_mode: Some("workspace-write"),
        })
        .expect("typed_ask_path_matching_normalizes_relative_and_absolute_workspace_paths");
    assert!(relative_path.requires_approval);
}

#[test]
fn typed_ask_path_matching_rejects_traversal_and_external_paths() {
    for (rule_path, path) in [
        ("src/a.rs", "../src/a.rs"),
        ("src/a.rs", "/workspace/src/../src/a.rs"),
        ("src/a.rs", "/src/a.rs"),
        ("../src/a.rs", "src/a.rs"),
        ("/src/a.rs", "src/a.rs"),
    ] {
        let engine = ExecPolicyEngine::with_rulesets(vec![
            Ruleset::user(vec![], vec![])
                .with_ask_rules(vec![ToolAskRule::file_path("edit_file", rule_path)]),
        ]);
        let decision = engine
            .check(ExecPolicyContext {
                command: "",
                cwd: "/workspace",
                tool: Some("edit_file"),
                path: Some(path),
                ask_for_approval: AskForApproval::OnFailure,
                sandbox_mode: Some("workspace-write"),
            })
            .expect("typed_ask_path_matching_rejects_traversal_and_external_paths");
        assert_eq!(
            decision.matched_rule, None,
            "rule {rule_path:?} and path {path:?} must not match"
        );
    }
}

#[test]
fn typed_ask_path_matching_accepts_windows_separators() {
    let engine = ExecPolicyEngine::with_rulesets(vec![
        Ruleset::user(vec![], vec![])
            .with_ask_rules(vec![ToolAskRule::file_path("edit_file", r"src\a.rs")]),
    ]);

    let decision = engine
        .check(ExecPolicyContext {
            command: "",
            cwd: r"C:\workspace",
            tool: Some("edit_file"),
            path: Some(r"C:\workspace\src\a.rs"),
            ask_for_approval: AskForApproval::OnFailure,
            sandbox_mode: Some("workspace-write"),
        })
        .expect("typed_ask_path_matching_accepts_windows_separators");

    assert!(decision.requires_approval);
}
