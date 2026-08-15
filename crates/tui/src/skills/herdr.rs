//! herdr runtime self-control skill — env-guarded loader.
//!
//! Mirrors herdrdev/herdr, which ships `skills/herdr/SKILL.md` gated behind
//! `HERDR_ENV`. Here the skill is only *offered* to the runtime when the host
//! runtime actually supports a herdr-style control plane, detected via one of:
//!
//! - `HERDR_ENV`
//! - `MIMOFAN_RUNTIME_ENV`
//!
//! When neither is set, [`herdr_skill_if_enabled`] returns `None` and the
//! runtime never sees this skill. This file is intentionally additive: it does
//! **not** touch any other skills module and does not change how the on-disk
//! `SKILL.md` discovery works.

use std::path::PathBuf;

use super::Skill;

/// The on-disk location of the bundled skill, relative to this source file.
///
/// The `SKILL.md` lives next to this loader at
/// `crates/tui/src/skills/herdr/SKILL.md`. We compute the path from
/// `file!()` so the loader tracks the source layout instead of hard-coding an
/// absolute path.
fn skill_markdown_path() -> PathBuf {
    let here = file!();
    let this_file = PathBuf::from(here);
    // `this_file` is `.../skills/herdr.rs`; the markdown is a sibling.
    let mut dir = this_file;
    dir.pop(); // drop `herdr.rs` → `.../skills/herdr`
    dir.push("SKILL.md");
    dir
}

/// True when the host runtime has explicitly enabled the herdr control plane.
///
/// Checks `HERDR_ENV` first, then falls back to `MIMOFAN_RUNTIME_ENV`. The
/// variable must be present and non-empty.
#[must_use]
pub fn herdr_env_enabled(env: &std::collections::HashMap<String, String>) -> bool {
    let set = |k: &str| env.get(k).is_some_and(|v| !v.is_empty());
    set("HERDR_ENV") || set("MIMOFAN_RUNTIME_ENV")
}

/// Returns the herdr self-control [`Skill`] only when the runtime is herdr-
/// enabled, otherwise `None`.
///
/// This is the single integration point: callers (e.g. the system-prompt
/// skill block) can append `herdr_skill_if_enabled()` to the active skill set
/// without affecting any other skill. It is pure and never fails — a missing
/// or malformed `SKILL.md` simply yields `None`.
#[must_use]
pub fn herdr_skill_if_enabled() -> Option<Skill> {
    let env: std::collections::HashMap<String, String> = std::env::vars().collect();
    if !herdr_env_enabled(&env) {
        return None;
    }
    let path = skill_markdown_path();
    let content = std::fs::read_to_string(&path).ok()?;
    let name = "herdr-runtime-self-control".to_string();

    // The bundled SKILL.md uses frontmatter; reconstruct the parsed fields we
    // need without depending on the full discovery parser (which expects a
    // directory scan). We mirror the frontmatter contract: `name` + a
    // `description` + the markdown `body`.
    let description = extract_frontmatter_field(&content, "description")
        .unwrap_or_else(|| "Agent self-coordination runtime primitives (pause/resume agents, inspect lifecycle, escalate to human), gated by HERDR_ENV.".to_string());
    let body = strip_frontmatter(&content);

    Some(Skill {
        name,
        description,
        body,
        path,
    })
}

/// Minimal frontmatter extraction matching the fields the bundled skill uses.
fn extract_frontmatter_field(content: &str, key: &str) -> Option<String> {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return None;
    }
    let rest = &trimmed[3..];
    let end = rest.find("---")?;
    let frontmatter = &rest[..end];
    for line in frontmatter.lines() {
        let line = line.trim();
        if let Some((k, v)) = line.split_once(':') {
            if k.trim().eq_ignore_ascii_case(key) {
                return Some(v.trim().to_string());
            }
        }
    }
    None
}

/// Return the markdown body with the leading `--- ... ---` frontmatter fence
/// stripped, so the skill's instructions (not the metadata) reach the model.
fn strip_frontmatter(content: &str) -> String {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return content.trim().to_string();
    }
    let rest = &trimmed[3..];
    if let Some(end) = rest.find("---") {
        rest[end + 3..].trim().to_string()
    } else {
        content.trim().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_when_no_env() {
        // Heroku-style guard: with neither var set the loader must stay inert.
        let env: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        assert!(!herdr_env_enabled(&env));
        // `herdr_skill_if_enabled` reads the real process env; this test only
        // covers the predicate logic to avoid depending on ambient env state.
    }

    #[test]
    fn enabled_with_herdr_env() {
        let mut env = std::collections::HashMap::new();
        env.insert("HERDR_ENV".to_string(), "1".to_string());
        assert!(herdr_env_enabled(&env));
    }

    #[test]
    fn enabled_with_mimofan_runtime_env() {
        let mut env = std::collections::HashMap::new();
        env.insert("MIMOFAN_RUNTIME_ENV".to_string(), "production".to_string());
        assert!(herdr_env_enabled(&env));
    }

    #[test]
    fn empty_var_is_not_enabled() {
        let mut env = std::collections::HashMap::new();
        env.insert("HERDR_ENV".to_string(), String::new());
        assert!(!herdr_env_enabled(&env));
    }
}
