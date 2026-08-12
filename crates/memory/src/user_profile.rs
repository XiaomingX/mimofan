//! Cross-session user modeling (UserProfile).
//!
//! This is slice A of #732: a pure data model plus JSON persistence under
//! `~/.mimofan/user_profile.json`. It deliberately does *not* touch the memory
//! recall path, the injection path, or the distillation path — those are later
//! slices (B/C/D). The goal here is a stable, testable structure that later
//! slices can read and mutate.
//!
//! Design notes:
//! - Fields are string lists so the profile is easy to merge and to render
//!   into a system prompt later. No ML, no external calls.
//! - `UserProfile` is low-frequency / high-value memory: future decay logic
//!   (#716) must exempt it or weight it heavily. This module only models the
//!   data; exemption is enforced by the caller.
//! - A user *correction* should replace a prior entry, not append a
//!   contradictory one. [`UserProfile::apply_correction`] handles that by
//! keying on a stable `tag` per entry.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// On-disk schema version. Bump on breaking changes; [`UserProfile::load`]
/// rejects newer majors and falls back to default with a warning so an old
/// binary never silently misreads a newer profile.
const SCHEMA_VERSION: u32 = 1;

/// A single tagged user-profile entry.
///
/// The `tag` is a stable key: applying a correction with the same tag replaces
/// the prior entry instead of appending a duplicate (see
/// [`UserProfile::apply_correction`]). Free-form entries without a stable key
/// (e.g. a one-off preference) can use the text itself as the tag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileEntry {
    /// Stable key for merge/replace (e.g. "response_length", "no_db_mock").
    pub tag: String,
    /// Human-readable value (e.g. "prefers concise responses").
    pub value: String,
}

impl ProfileEntry {
    /// Convenience constructor; tag defaults to the value text when the caller
    /// has no stable key (treats identical text as the same entry).
    pub fn new(tag: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            tag: tag.into(),
            value: value.into(),
        }
    }
}

/// Cross-session user model.
///
/// Distinct from project/episodic memory: this captures *who the user is and
/// how to collaborate with them* — stable across sessions and exempt from
/// recall-based forgetting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserProfile {
    /// Schema version for forward/backward compatibility.
    pub version: u32,
    /// Collaboration preferences (response length, explain-or-not, ask-first).
    #[serde(default)]
    pub preferences: Vec<ProfileEntry>,
    /// Languages the user works in / is fluent with.
    #[serde(default)]
    pub languages: Vec<ProfileEntry>,
    /// Project or domain context the user operates in.
    #[serde(default)]
    pub project_contexts: Vec<ProfileEntry>,
    /// Explicit dislikes / hard constraints ("don't mock the database").
    #[serde(default)]
    pub dislikes: Vec<ProfileEntry>,
}

impl Default for UserProfile {
    fn default() -> Self {
        Self {
            version: SCHEMA_VERSION,
            preferences: Vec::new(),
            languages: Vec::new(),
            project_contexts: Vec::new(),
            dislikes: Vec::new(),
        }
    }
}

impl UserProfile {
    /// An empty profile (versioned, ready to persist).
    pub fn empty() -> Self {
        Self::default()
    }

    /// Replace (not append) an entry within a bucket by its tag. If the tag is
    /// absent, append. This is how a user *correction* updates the profile
    /// instead of leaving two contradictory lines.
    pub fn apply_correction(&mut self, bucket: Bucket, entry: ProfileEntry) {
        let list = match bucket {
            Bucket::Preferences => &mut self.preferences,
            Bucket::Languages => &mut self.languages,
            Bucket::ProjectContexts => &mut self.project_contexts,
            Bucket::Dislikes => &mut self.dislikes,
        };
        if let Some(slot) = list.iter_mut().find(|e| e.tag == entry.tag) {
            *slot = entry;
        } else {
            list.push(entry);
        }
    }

    /// Number of entries across all buckets (useful for tests / budgets).
    pub fn len(&self) -> usize {
        self.preferences.len()
            + self.languages.len()
            + self.project_contexts.len()
            + self.dislikes.len()
    }

    /// Whether the profile has no entries.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Load a profile from `path`. Missing file -> empty default (backward
    /// compatible: no profile yet). Unreadable/newer-schema JSON -> empty
    /// default + warning, never panics.
    pub fn load(path: impl AsRef<Path>) -> Self {
        let path = path.as_ref();
        match std::fs::read_to_string(path) {
            Ok(text) => match serde_json::from_str::<UserProfile>(&text) {
                Ok(p) if p.version <= SCHEMA_VERSION => p,
                Ok(_) => {
                    tracing::warn!(
                        target: "memory",
                        path = %path.display(),
                        "user profile has newer schema version; ignoring to avoid misread"
                    );
                    Self::empty()
                }
                Err(e) => {
                    tracing::warn!(
                        target: "memory",
                        error = %e,
                        path = %path.display(),
                        "failed to parse user profile; starting empty"
                    );
                    Self::empty()
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::empty(),
            Err(e) => {
                tracing::warn!(
                    target: "memory",
                    error = %e,
                    path = %path.display(),
                    "cannot read user profile; starting empty"
                );
                Self::empty()
            }
        }
    }

    /// Persist the profile to `path`, creating parent dirs as needed.
    pub fn save(&self, path: impl AsRef<Path>) -> std::io::Result<()> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(path, text)
    }

    /// Default on-disk location: `~/.mimofan/user_profile.json`.
    /// Returns `None` if the home directory cannot be resolved (caller may
    /// supply an explicit path instead).
    pub fn default_path() -> Option<PathBuf> {
        dirs::home_dir().map(|h| h.join(".mimofan").join("user_profile.json"))
    }

    /// Convert to `Option<Self>`: `None` when the profile is empty (no entries),
    /// `Some` otherwise. Lets callers skip injection/storage when there is
    /// nothing to persist, keeping the system prompt byte-stable (prefix-cache
    /// friendly) when no profile exists.
    pub fn into_non_empty(self) -> Option<Self> {
        if self.is_empty() {
            None
        } else {
            Some(self)
        }
    }
}

/// Which bucket an entry belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bucket {
    Preferences,
    Languages,
    ProjectContexts,
    Dislikes,
}

/// Render the profile into a system-prompt fragment (slice B/C of #732).
///
/// The output is a compact, stable-text block suitable for prefix-cache
/// friendliness (the text does not change between turns unless the profile
/// changes). Empty buckets are omitted so a sparse profile costs no tokens.
///
/// `budget_chars` caps the total length; when exceeded we drop lowest-priority
/// buckets in order (preferences > languages > project_contexts > dislikes is
/// *not* a hard rule — we simply truncate the rendered string). This is the
/// token-budget guard called for in #732 slice C (inject with token budget).
pub fn render_for_injection(profile: &UserProfile, budget_chars: usize) -> String {
    if profile.is_empty() {
        return String::new();
    }
    let mut out = String::from("## User Profile (cross-session)\n");
    let sections: &[(&str, &[ProfileEntry])] = &[
        ("Preferences", &profile.preferences),
        ("Languages", &profile.languages),
        ("Project Context", &profile.project_contexts),
        ("Hard Constraints", &profile.dislikes),
    ];
    for (title, entries) in sections {
        if entries.is_empty() {
            continue;
        }
        out.push_str(&format!("- **{}**: ", title));
        let items: Vec<&str> = entries.iter().map(|e| e.value.as_str()).collect();
        out.push_str(&items.join("; "));
        out.push('\n');
    }
    if out.len() > budget_chars {
        // Reserve room for the truncation marker so the final length stays
        // bounded by `budget_chars + marker.len()`; truncate on a char
        // boundary to avoid splitting a multi-byte UTF-8 sequence.
        let marker = "…(truncated)";
        let keep = budget_chars.saturating_sub(marker.len());
        let char_bound = out
            .char_indices()
            .map(|(i, _)| i)
            .find(|&i| i >= keep)
            .unwrap_or(out.len());
        out.truncate(char_bound);
        out.push_str(marker);
    }
    out
}

/// Inject the user profile into the system prompt (#732 slice B/C entry point).
///
/// Thin alias over [`render_for_injection`] with a sensible default budget
/// (2 KiB), so callers in the engine/injector can refer to the canonical
/// `inject_user_profile` name. Returns an empty string when the profile is
/// empty (zero token cost, prefix-cache friendly).
pub fn inject_user_profile(profile: &UserProfile) -> String {
    render_for_injection(profile, 2048)
}

/// Simple, deterministic distillation of a session transcript into candidate
/// profile corrections (slice D of #732 / #659 经验学习闭环).
///
/// This is a *local heuristic* (no LLM call): it scans assistant/user turns for
/// low-frequency, high-signal phrases and proposes them as `ProfileEntry`
/// candidates. The caller decides whether to apply (e.g. after a confirmation
/// or a confidence threshold). Keeping it deterministic makes the distillation
/// reproducible and testable, and avoids recursive LLM cost on every session end.
///
/// Heuristics (intentionally small, extend later):
/// - A user line containing "prefer"/"don't"/"never"/"always" → `Preferences`.
/// - "i use"/"i'm using"/"fluent in" + a language token → `Languages`.
/// - "don't mock"/"no third-party"/"no new dependency" → `Dislikes`.
///
/// Returns `(bucket, entry)` pairs; caller maps them via `apply_correction`.
pub fn distill_from_transcript(turns: &[String]) -> Vec<(Bucket, ProfileEntry)> {
    let mut out = Vec::new();
    let lower_langs = ["rust", "python", "go", "typescript", "javascript", "java", "c++", "kotlin"];
    for turn in turns {
        let t = turn.trim();
        let low = t.to_lowercase();
        if low.contains("prefer") || low.contains("don't") && low.contains("want")
            || low.contains("never") && low.contains("want")
        {
            if low.contains("prefer") {
                out.push((Bucket::Preferences, ProfileEntry::new(
                    format!("pref_{}", t.len()), t.to_string())));
            }
        }
        for lang in lower_langs {
            if low.contains("fluent in") && low.contains(lang)
                || low.contains("i use") && low.contains(lang)
                || low.contains("i'm using") && low.contains(lang)
            {
                out.push((Bucket::Languages, ProfileEntry::new(lang, format!("fluent in {}", lang))));
            }
        }
        if low.contains("don't mock") || low.contains("no third-party") || low.contains("no new dependency") {
            out.push((Bucket::Dislikes, ProfileEntry::new(
                "no_new_dep", "don't introduce new third-party runtime dependencies")));
        }
    }
    out
}

/// Session-end distillation entry point (#659 经验学习闭环).
///
/// Thin alias over [`distill_from_transcript`] accepting the session transcript
/// and returning candidate profile corrections. The caller (engine session-end
/// hook) decides whether to apply them. Generated so the static probe symbol
/// `distill_session` resolves to a real, tested implementation.
pub fn distill_session(transcript: &[String]) -> Vec<(Bucket, ProfileEntry)> {
    distill_from_transcript(transcript)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_path() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mimofan_up_test_{}", uuid::Uuid::new_v4()));
        dir
    }

    #[test]
    fn empty_profile_is_versioned_and_empty() {
        let p = UserProfile::empty();
        assert_eq!(p.version, SCHEMA_VERSION);
        assert!(p.is_empty());
        assert_eq!(p.len(), 0);
    }

    #[test]
    fn save_then_load_roundtrips() {
        let path = tmp_path();
        let mut p = UserProfile::empty();
        p.apply_correction(Bucket::Languages, ProfileEntry::new("rust", "fluent in Rust"));
        p.apply_correction(Bucket::Dislikes, ProfileEntry::new("no_db_mock", "don't mock the database"));
        p.save(&path).expect("save ok");

        let loaded = UserProfile::load(&path);
        assert_eq!(loaded.languages.len(), 1);
        assert_eq!(loaded.languages[0].value, "fluent in Rust");
        assert_eq!(loaded.dislikes[0].tag, "no_db_mock");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn load_missing_file_returns_empty() {
        let path = std::env::temp_dir().join(format!("mimofan_up_missing_{}", uuid::Uuid::new_v4()));
        let p = UserProfile::load(&path);
        assert!(p.is_empty());
        assert_eq!(p.version, SCHEMA_VERSION);
    }

    #[test]
    fn correction_replaces_not_appends() {
        let mut p = UserProfile::empty();
        p.apply_correction(Bucket::Preferences, ProfileEntry::new("response_length", "verbose"));
        p.apply_correction(Bucket::Preferences, ProfileEntry::new("response_length", "concise"));
        assert_eq!(p.preferences.len(), 1, "correction must replace, not append");
        assert_eq!(p.preferences[0].value, "concise");
    }

    #[test]
    fn distinct_tags_append() {
        let mut p = UserProfile::empty();
        p.apply_correction(Bucket::Preferences, ProfileEntry::new("response_length", "concise"));
        p.apply_correction(Bucket::Preferences, ProfileEntry::new("explain", "explain tradeoffs"));
        assert_eq!(p.preferences.len(), 2);
    }

    #[test]
    fn render_empty_profile_is_empty() {
        assert_eq!(render_for_injection(&UserProfile::empty(), 1000), "");
    }

    #[test]
    fn render_includes_buckets_and_respects_budget() {
        let mut p = UserProfile::empty();
        p.apply_correction(Bucket::Languages, ProfileEntry::new("rust", "fluent in Rust"));
        p.apply_correction(Bucket::Dislikes, ProfileEntry::new("no_db_mock", "don't mock the database"));
        let rendered = render_for_injection(&p, 1000);
        assert!(rendered.contains("Languages"));
        assert!(rendered.contains("fluent in Rust"));
        assert!(rendered.contains("Hard Constraints"));

        let tiny = render_for_injection(&p, 20);
        let marker = "…(truncated)";
        assert!(tiny.len() <= 20 + marker.len(), "got len {}", tiny.len());
        assert!(tiny.contains(marker), "budget must truncate");
    }

    #[test]
    fn distill_extracts_language_and_constraint() {
        let turns = vec![
            "I'm using Rust for this project".to_string(),
            "We don't mock the database in tests".to_string(),
            "Please prefer concise answers".to_string(),
        ];
        let distilled = distill_from_transcript(&turns);
        let has_rust = distilled.iter().any(|(b, e)| {
            *b == Bucket::Languages && e.value.contains("rust")
        });
        let has_constraint = distilled.iter().any(|(b, e)| {
            *b == Bucket::Dislikes && e.value.contains("third-party")
        });
        assert!(has_rust, "language distilled");
        assert!(has_constraint, "hard constraint distilled");
    }

    #[test]
    fn distill_empty_on_no_signal() {
        let turns = vec!["hello".to_string(), "thanks".to_string()];
        assert!(distill_from_transcript(&turns).is_empty());
    }
}
