//! `/change` command — show a changelog entry, translated to the user's
//! locale when it is not English.
//!
//! Usage: `/change [version]`
//!
//! Uses the mimofan changelog embedded at compile time. With no argument,
//! extracts the most recent section. With a version argument like `0.8.32`,
//! extracts that specific version's section. When the UI locale is not
//! English and the current session can reach a model, the command also fires a
//! `SendMessage` action that asks the model to translate the changelog into
//! the user's language.

use crate::localization::{Locale, MessageId, tr};
use crate::tui::app::{App, AppAction};

use super::CommandResult;

/// Maximum length of the changelog excerpt we'll show inline (characters).
/// If the changelog section exceeds this, we truncate and show a notice.
/// 4096 chars is large enough for most version entries.
const MAX_INLINE_CHANGELOG_CHARS: usize = 4096;
const MIMOFAN_CHANGELOG: &str = include_str!("../../../../CHANGELOG.md");

/// Execute the `/change` command.
///
/// If `version` is `None`, shows the latest non-empty version section.
/// If `version` is `Some(v)`, shows the section for that version.
pub fn change(app: &mut App, version: Option<&str>) -> CommandResult {
    let section = if let Some(ver) = version {
        let ver = ver.trim();
        if ver.is_empty() {
            extract_latest_changelog_section(MIMOFAN_CHANGELOG)
        } else {
            extract_changelog_section_by_version(MIMOFAN_CHANGELOG, ver)
        }
    } else {
        extract_latest_changelog_section(MIMOFAN_CHANGELOG)
    };

    let latest_section = match section {
        Some(s) => s,
        None => {
            let msg = if let Some(ver) = version {
                let ver = ver.trim();
                if ver.is_empty() {
                    "Could not find a version section in the bundled mimofan changelog. \
                     Expected a line starting with `## [`."
                        .to_string()
                } else {
                    format!("Could not find version \"{ver}\" in the bundled mimofan changelog.")
                }
            } else {
                "Could not find a version section in the bundled mimofan changelog. \
                 Expected a line starting with `## [`."
                    .to_string()
            };
            return CommandResult::error(msg);
        }
    };

    let locale = app.ui_locale;
    let header = tr(locale, MessageId::CmdChangeHeader);

    let prev_hint = if let Some(prev_ver) = previous_version_hint(MIMOFAN_CHANGELOG, version) {
        let template = tr(locale, MessageId::CmdChangePreviousVersion);
        format!("\n\n{}", template.replace("{version}", &prev_ver))
    } else {
        String::new()
    };

    let section_text = inline_changelog_section(&latest_section);

    // If the user's locale is English, just display.
    // Otherwise, also ask the model to translate.
    if locale == Locale::ZhHans {
        CommandResult::message(format!(
            "{header}\n─────────────────────────────\n{section_text}{prev_hint}"
        ))
    } else if app.offline_mode || app.onboarding_needs_api_key {
        let fallback = tr(locale, MessageId::CmdChangeTranslationUnavailable);
        CommandResult::message(format!(
            "{header}\n\
─────────────────────────────\n\
{fallback}\n\n\
{section_text}{prev_hint}"
        ))
    } else {
        let queued = tr(locale, MessageId::CmdChangeTranslationQueued);
        let display_text = format!(
            "{header}\n\
─────────────────────────────\n\
{queued}\n\n\
{section_text}{prev_hint}"
        );
        let translation_source = format!("{latest_section}{prev_hint}");
        let lang_name = match locale {
            Locale::ZhHans => "Simplified Chinese (中文)",
            Locale::ZhHans => "Traditional Chinese (繁體中文)",
            // Fallback — should never reach here since we check En above.
            Locale::ZhHans => "English",
        };

        let translation_prompt = format!(
            "Translate the following changelog into {lang_name}. \
             Keep all markdown formatting, version numbers, dates, \
             contributor names, and code references intact. \
             Output ONLY the translated changelog, no preamble or commentary.\n\n\
             {translation_source}"
        );

        CommandResult::with_message_and_action(
            display_text,
            AppAction::SendMessage(translation_prompt),
        )
    }
}

fn inline_changelog_section(section: &str) -> String {
    if section.len() <= MAX_INLINE_CHANGELOG_CHARS {
        return section.to_string();
    }

    let truncated: String = section.chars().take(MAX_INLINE_CHANGELOG_CHARS).collect();
    format!(
        "{truncated}\n\
\n\
[... {} characters omitted from the bundled mimofan changelog]",
        section.len() - MAX_INLINE_CHANGELOG_CHARS
    )
}

/// Extract the latest version section from CHANGELOG.md content.
///
/// Looks for the first `## [version] - date` heading and returns all lines
/// from that heading up to the next `## [` heading (or end of file).
/// Leading and trailing whitespace is trimmed.
///
/// Skips empty sections (e.g. `## [Unreleased]` with no content) to find
/// the first section that actually has content.
fn extract_latest_changelog_section(content: &str) -> Option<String> {
    let lines: Vec<&str> = content.lines().collect();

    // Find the first `## [` heading index
    let first_idx = {
        let mut idx = None;
        for (i, line) in lines.iter().enumerate() {
            if line.trim().starts_with("## [") {
                idx = Some(i);
                break;
            }
        }
        idx?
    };

    // Starting from `first_idx`, walk through headings until we find a
    // section with non-empty content.
    let mut pos = first_idx;
    loop {
        let end = lines
            .iter()
            .enumerate()
            .skip(pos + 1)
            .find(|(_, line)| line.trim().starts_with("## ["))
            .map_or(lines.len(), |(i, _)| i);

        if section_has_body_content(&lines[pos + 1..end]) {
            return Some(lines[pos..end].join("\n").trim().to_string());
        }

        // Empty section — try the next heading (if any)
        if end >= lines.len() {
            return None;
        }
        pos = end;
    }
}

/// Extract a specific version section from CHANGELOG.md content.
///
/// Looks for `## [<version>]` or `## [<version> - date]` and returns all
/// lines from that heading up to the next `## [` heading (or end of file).
fn extract_changelog_section_by_version(content: &str, version: &str) -> Option<String> {
    let lines: Vec<&str> = content.lines().collect();
    let mut start_idx: Option<usize> = None;

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("## [") {
            // Check if this heading matches the requested version.
            // Format: `## [0.8.32] - 2026-05-12` or `## [0.8.32]`
            let bracket_end = trimmed.find(']')?;
            let heading_ver = &trimmed[4..bracket_end]; // skip "## ["
            if heading_ver == version {
                start_idx = Some(i);
                break;
            }
        }
    }

    let start = start_idx?;

    let end = lines
        .iter()
        .enumerate()
        .skip(start + 1)
        .find(|(_, line)| line.trim().starts_with("## ["))
        .map_or(lines.len(), |(i, _)| i);

    if !section_has_body_content(&lines[start + 1..end]) {
        return None;
    }

    Some(lines[start..end].join("\n").trim().to_string())
}

/// Extract the version number of the section immediately preceding the latest
/// non-empty section in the changelog.
///
/// Walks past empty sections (e.g. `## [Unreleased]`) the same way
/// [`extract_latest_changelog_section`] does, then returns the version from
/// the next `## [version]` heading after the first contentful section.
fn extract_previous_version_number(content: &str) -> Option<String> {
    let lines: Vec<&str> = content.lines().collect();
    let first_idx = lines.iter().position(|l| l.trim().starts_with("## ["))?;

    let mut pos = first_idx;
    loop {
        let end = lines
            .iter()
            .enumerate()
            .skip(pos + 1)
            .find(|(_, l)| l.trim().starts_with("## ["))
            .map_or(lines.len(), |(i, _)| i);

        if section_has_body_content(&lines[pos + 1..end]) {
            // Found the latest contentful section heading at `pos`.
            return next_contentful_version_after(&lines, end);
        }

        if end >= lines.len() {
            return None;
        }
        pos = end;
    }
}

fn section_has_body_content(lines: &[&str]) -> bool {
    lines.iter().any(|line| !line.trim().is_empty())
}

fn previous_version_hint(content: &str, version: Option<&str>) -> Option<String> {
    match version.map(str::trim).filter(|v| !v.is_empty()) {
        Some(version) => extract_previous_version_number_after_version(content, version),
        None => extract_previous_version_number(content),
    }
}

fn extract_previous_version_number_after_version(content: &str, version: &str) -> Option<String> {
    let lines: Vec<&str> = content.lines().collect();
    let current_start = lines.iter().position(|line| {
        let trimmed = line.trim();
        trimmed
            .strip_prefix("## [")
            .and_then(|rest| rest.split_once(']'))
            .is_some_and(|(heading_ver, _)| heading_ver == version)
    })?;

    let current_end = lines
        .iter()
        .enumerate()
        .skip(current_start + 1)
        .find(|(_, line)| line.trim().starts_with("## ["))
        .map_or(lines.len(), |(i, _)| i);

    next_contentful_version_after(&lines, current_end)
}

fn next_contentful_version_after(lines: &[&str], mut pos: usize) -> Option<String> {
    while pos < lines.len() {
        let heading = lines[pos].trim();
        if !heading.starts_with("## [") {
            pos += 1;
            continue;
        }

        let end = lines
            .iter()
            .enumerate()
            .skip(pos + 1)
            .find(|(_, line)| line.trim().starts_with("## ["))
            .map_or(lines.len(), |(i, _)| i);

        if section_has_body_content(&lines[pos + 1..end]) {
            let bracket_end = heading.find(']')?;
            return Some(heading[4..bracket_end].to_string());
        }

        pos = end;
    }

    None
}

#[cfg(test)]
mod tests {}
