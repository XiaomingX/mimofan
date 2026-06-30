use super::CommandResult;
use crate::commands::traits::{CommandInfo, RegisterCommand};
use crate::localization::MessageId;
use crate::tui::app::{App, AppAction};

const SECURITY_POLICY_URL: &str = "https://github.com/XiaomingX/mimofan/security/policy";

pub(in crate::commands) const COMMAND_INFO: CommandInfo = CommandInfo {
    name: "feedback",
    aliases: &[],
    usage: "/feedback [bug|feature|security]",
    description_id: MessageId::CmdFeedbackDescription,
};

pub(in crate::commands) struct FeedbackCmd;

impl RegisterCommand for FeedbackCmd {
    fn info() -> &'static CommandInfo {
        &COMMAND_INFO
    }

    fn execute(app: &mut App, arg: Option<&str>) -> CommandResult {
        feedback(app, arg)
    }
}

pub fn feedback(_app: &mut App, arg: Option<&str>) -> CommandResult {
    let raw = arg.map(str::trim).unwrap_or("");
    if raw.is_empty() {
        return CommandResult::action(AppAction::OpenFeedbackPicker);
    }
    if matches!(raw, "help" | "--help" | "-h") {
        return CommandResult::message(feedback_help());
    }

    let kind = match parse_feedback_kind(raw) {
        Some(parsed) => parsed,
        None => {
            return CommandResult::error(
                "Unknown feedback type. Use `/feedback` to list feedback options.",
            );
        }
    };

    if matches!(kind, FeedbackKind::Security) {
        return CommandResult::with_message_and_action(
            format!(
                "Review the project's security policy before reporting a vulnerability.\n\n\
                 Trying to open it in your browser. If that fails, open this URL manually:\n\n\
                 {SECURITY_POLICY_URL}\n\n\
                 Do not include sensitive security details in a public issue.",
            ),
            AppAction::OpenExternalUrl {
                url: SECURITY_POLICY_URL.to_string(),
                label: "GitHub security policy".to_string(),
            },
        );
    }

    let url = kind.issue_url();
    let mut message = format!(
        "Trying to open GitHub {} template in your browser. If that fails, open this URL manually:\n\n{}",
        kind.label().to_ascii_lowercase(),
        url,
    );
    if matches!(kind, FeedbackKind::Bug) {
        message.push_str("\n\n");
        message.push_str(bug_report_diagnostics_hint());
    }

    CommandResult::with_message_and_action(
        message,
        AppAction::OpenExternalUrl {
            url,
            label: format!("GitHub {}", kind.label().to_ascii_lowercase()),
        },
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FeedbackKind {
    Bug,
    Feature,
    Security,
}

impl FeedbackKind {
    fn label(self) -> &'static str {
        match self {
            Self::Bug => "Bug report",
            Self::Feature => "Feature request",
            Self::Security => "Security vulnerability",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::Bug => "Report a problem or regression",
            Self::Feature => "Suggest an idea or improvement",
            Self::Security => "Review the security policy",
        }
    }

    fn issue_url_base(self) -> &'static str {
        match self {
            Self::Bug => "https://github.com/XiaomingX/mimofan/issues/new?template=bug_report.md",
            Self::Feature => {
                "https://github.com/XiaomingX/mimofan/issues/new?template=feature_request.md"
            }
            Self::Security => SECURITY_POLICY_URL,
        }
    }

    fn issue_url(self) -> String {
        self.issue_url_base().to_string()
    }
}

fn feedback_help() -> String {
    let rows = [
        ("1", FeedbackKind::Bug),
        ("2", FeedbackKind::Feature),
        ("3", FeedbackKind::Security),
    ];
    let mut message = String::from("Choose a feedback type:\n\n");
    for (number, kind) in rows {
        message.push_str(&format!(
            "{number}. {}    {}\n",
            kind.label(),
            kind.description()
        ));
    }
    message.push_str("\nUsage:\n");
    for (number, kind) in rows {
        message.push_str(&format!("/feedback {number}    {}\n", kind.label()));
    }
    message.push_str("/feedback bug\n");
    message.push_str("/feedback feature\n");
    message.push_str("/feedback security\n");
    message
}

fn bug_report_diagnostics_hint() -> &'static str {
    "Before filing, first check whether this looks like a model issue or an environment/tool issue: \
     command exit, network/service, sandbox/approval, missing dependency/path, timeout, or an unclosed turn. \
     Include the mimofan version, OS/terminal, the tool name, and redacted timestamps or log handles when available. \
     Do not paste prompts, secrets, raw command output, full local paths, or conversation transcripts."
}

fn parse_feedback_kind(input: &str) -> Option<FeedbackKind> {
    Some(match input.to_ascii_lowercase().as_str() {
        "1" | "bug" | "bug-report" | "bug_report" => FeedbackKind::Bug,
        "2" | "feature" | "feature-request" | "feature_request" | "enhancement" => {
            FeedbackKind::Feature
        }
        "3" | "security" | "vulnerability" | "private" => FeedbackKind::Security,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {}
