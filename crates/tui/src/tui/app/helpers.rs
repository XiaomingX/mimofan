//! Helper functions for the TUI application.

use std::path::PathBuf;

use super::state::{AppMode, EffectiveModePolicy, ModeSessionPrefs, OnboardingState};
use crate::config::Config;
use crate::tui::approval::ApprovalMode;

/// Count the number of Unicode scalar values in a string.
pub(crate) fn char_count(text: &str) -> usize {
    text.chars().count()
}

/// Get the byte index of the character at the given character index.
pub(crate) fn byte_index_at_char(text: &str, char_index: usize) -> usize {
    if char_index == 0 {
        return 0;
    }
    text.char_indices()
        .nth(char_index)
        .map(|(idx, _)| idx)
        .unwrap_or_else(|| text.len())
}

/// Remove the character at the given character index from the string.
/// Returns true if the character was removed, false if the index was out of bounds.
pub(crate) fn remove_char_at(text: &mut String, char_index: usize) -> bool {
    let start = byte_index_at_char(text, char_index);
    if start >= text.len() {
        return false;
    }
    let ch = text[start..]
        .chars()
        .next()
        .expect("start < text.len() ensures non-empty slice");
    let end = start + ch.len_utf8();
    text.replace_range(start..end, "");
    true
}

/// Normalize line endings in pasted text.
pub(crate) fn normalize_paste_text(text: &str) -> String {
    if text.contains('\r') {
        text.replace("\r\n", "\n").replace('\r', "\n")
    } else {
        text.to_string()
    }
}

/// Sanitize API key text by removing control characters.
pub(crate) fn sanitize_api_key_text(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).collect()
}

/// Default setting for composer arrows scrolling behavior.
pub(crate) fn default_composer_arrows_scroll(use_mouse_capture: bool) -> bool {
    !use_mouse_capture
}

/// Resolve the skills directory based on workspace and config.
pub(crate) fn resolve_skills_dir(
    workspace: &std::path::Path,
    global_skills_dir: &std::path::Path,
    config: &Config,
) -> PathBuf {
    if config.skills_config().scan_mimofan_only() {
        if config.skills_dir.is_some() {
            return global_skills_dir.to_path_buf();
        }
        if let Some(mimofan_skills_dir) = crate::skills::mimofan_workspace_skills_dir(workspace) {
            return mimofan_skills_dir;
        }
        return global_skills_dir.to_path_buf();
    }

    let agents_skills_dir = workspace.join(".agents").join("skills");
    if agents_skills_dir.exists() {
        return agents_skills_dir;
    }

    let local_skills_dir = workspace.join("skills");
    if local_skills_dir.exists() {
        return local_skills_dir;
    }

    if config.skills_dir.is_none()
        && let Some(global_agents) = crate::skills::agents_global_skills_dir()
        && global_agents.exists()
    {
        return global_agents;
    }

    global_skills_dir.to_path_buf()
}

/// Check if input looks like a slash command.
pub(crate) fn looks_like_slash_command_input(input: &str) -> bool {
    let trimmed = input.trim_start();
    // `$skillname` at the start of input is treated like a slash command so the
    // skill-completion menu appears.
    let Some(rest) = trimmed
        .strip_prefix('/')
        .or_else(|| trimmed.strip_prefix('$'))
    else {
        return false;
    };
    if rest.chars().next().is_some_and(|ch| ch.is_whitespace()) {
        return false;
    }
    let Some(command) = rest.split_whitespace().next() else {
        return rest.is_empty();
    };

    !command.contains('/')
}

/// Parse shell command from bang input (e.g., "! ls").
pub(crate) fn shell_command_from_bang_input(input: &str) -> Result<Option<&str>, &'static str> {
    let Some(rest) = input.trim_start().strip_prefix('!') else {
        return Ok(None);
    };
    let command = rest.trim();
    if command.is_empty() {
        return Err("Usage: ! <shell command>");
    }
    Ok(Some(command))
}

/// Determine initial onboarding state based on configuration.
pub(crate) fn initial_onboarding_state(
    skip_onboarding: bool,
    was_onboarded: bool,
    needs_api_key: bool,
    needs_workspace_trust: bool,
) -> OnboardingState {
    if skip_onboarding || (was_onboarded && !needs_api_key && !needs_workspace_trust) {
        return OnboardingState::None;
    }

    if was_onboarded && needs_api_key {
        OnboardingState::ApiKey
    } else if was_onboarded && needs_workspace_trust {
        OnboardingState::TrustDirectory
    } else {
        OnboardingState::Welcome
    }
}

/// Check if onboarding is at the workspace trust gate.
pub(crate) fn onboarding_is_workspace_trust_gate(
    skip_onboarding: bool,
    was_onboarded: bool,
    needs_api_key: bool,
    needs_workspace_trust: bool,
) -> bool {
    !skip_onboarding && was_onboarded && !needs_api_key && needs_workspace_trust
}

/// Resolve a mode's effective permission policy from the durable Agent baseline.
///
/// This is the single source of truth for the mode/permission table (#3386):
/// - `Plan`   → read-only: no shell, no trust, `Suggest` approvals.
/// - `Agent`  → the user's durable baseline (`prefs`).
/// - `Yolo`   → full authority: shell + trust + `Auto` approvals.
///
/// Pure and side-effect free so it can be unit-tested directly and reused by
/// any policy consumer.
pub(crate) fn base_policy_for_mode(mode: AppMode, prefs: &ModeSessionPrefs) -> EffectiveModePolicy {
    match mode {
        AppMode::Plan => EffectiveModePolicy {
            mode,
            allow_shell: false,
            trust_mode: false,
            approval_mode: ApprovalMode::Suggest,
            auto_approve: false,
        },
        AppMode::Agent => EffectiveModePolicy {
            mode,
            allow_shell: prefs.agent_allow_shell,
            trust_mode: prefs.agent_trust_mode,
            approval_mode: prefs.agent_approval_mode,
            auto_approve: false,
        },
        AppMode::Yolo => EffectiveModePolicy {
            mode,
            allow_shell: true,
            trust_mode: true,
            approval_mode: ApprovalMode::Auto,
            auto_approve: true,
        },
    }
}

/// Format media attachment reference string.
pub fn media_attachment_reference(
    kind: &str,
    path: &std::path::Path,
    description: Option<&str>,
) -> String {
    match description {
        Some(description) if !description.trim().is_empty() => {
            format!(
                "[Attached {kind}: {} at {}]",
                description.trim(),
                path.display()
            )
        }
        _ => format!("[Attached {kind}: {}]", path.display()),
    }
}
