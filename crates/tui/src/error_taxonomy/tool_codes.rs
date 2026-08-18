//! Fine-grained, machine-readable error codes for individual file tools.
//!
//! These codes let the model branch on a stable, parseable token instead of
//! pattern-matching English error prose. Each [`ToolCode`] is retryable by
//! design: the structural failures it names (prior-read missing, ambiguous
//! match, non-regular target, target not found) are conditions the model can
//! resolve by changing its own arguments or by re-reading state — not
//! unrecoverable engine faults.
//!
//! Issue #872: previously reported as "implemented", but empirically the
//! whole crate had zero references to a tool-code taxonomy. This module is
//! the real, wired-in implementation.

/// Machine-readable failure mode for a single tool invocation.
///
/// Serializes as SCREAMING_SNAKE_CASE so the wire form matches the `[CODE]`
/// prefix convention used when attaching a code to a plain `ToolError`
/// message (see `crate::tools::file::err_with_code`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ToolCode {
    /// `edit_file` (or another mutating tool) was attempted before the target
    /// file — or the specific target lines — had been read in this session.
    EditRequiresPriorRead,
    /// The file was read, but its on-disk contents changed before the edit
    /// was applied. Retrying after a fresh `read_file` recovers.
    FileChangedSinceRead,
    /// A search/anchor in `edit_file` matched more than one location, so the
    /// edit was rejected to avoid silently rewriting the wrong region.
    AmbiguousMatch,
    /// The target is a FIFO, socket, character device, or directory rather
    /// than a regular file, so opening it for `read_file` would hang or be
    /// meaningless.
    TargetNotRegularFile,
    /// The requested path does not exist on disk.
    TargetNotFound,
}

impl ToolCode {
    /// These structural failures are all conditions the model can resolve by
    /// changing its own next action (read first, narrow the search, pick a
    /// real file), so every variant is retryable.
    #[must_use]
    pub fn is_retryable(self) -> bool {
        true
    }

    /// The stable wire string, e.g. `"EDIT_REQUIRES_PRIOR_READ"`.
    ///
    /// This is the token that travels ahead of the human-readable message in
    /// the `[CODE]` prefix convention.
    #[must_use]
    pub fn as_code_str(self) -> &'static str {
        match self {
            Self::EditRequiresPriorRead => "EDIT_REQUIRES_PRIOR_READ",
            Self::FileChangedSinceRead => "FILE_CHANGED_SINCE_READ",
            Self::AmbiguousMatch => "AMBIGUOUS_MATCH",
            Self::TargetNotRegularFile => "TARGET_NOT_REGULAR_FILE",
            Self::TargetNotFound => "TARGET_NOT_FOUND",
        }
    }
}

impl std::fmt::Display for ToolCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_code_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_code_has_a_stable_string_and_is_retryable() {
        for code in [
            ToolCode::EditRequiresPriorRead,
            ToolCode::FileChangedSinceRead,
            ToolCode::AmbiguousMatch,
            ToolCode::TargetNotRegularFile,
            ToolCode::TargetNotFound,
        ] {
            assert!(code.is_retryable(), "{code} must be retryable");
            // The string must be stable and non-empty so downstream retries
            // key on it deterministically.
            assert!(!code.as_code_str().is_empty(), "{code:?} needs a code string");
        }
    }

    #[test]
    fn code_strings_match_the_wire_prefix_contract() {
        assert_eq!(ToolCode::EditRequiresPriorRead.as_code_str(), "EDIT_REQUIRES_PRIOR_READ");
        assert_eq!(ToolCode::FileChangedSinceRead.as_code_str(), "FILE_CHANGED_SINCE_READ");
        assert_eq!(ToolCode::AmbiguousMatch.as_code_str(), "AMBIGUOUS_MATCH");
        assert_eq!(ToolCode::TargetNotRegularFile.as_code_str(), "TARGET_NOT_REGULAR_FILE");
        assert_eq!(ToolCode::TargetNotFound.as_code_str(), "TARGET_NOT_FOUND");
    }

    #[test]
    fn display_round_trips_through_code_str() {
        let code = ToolCode::AmbiguousMatch;
        assert_eq!(code.to_string(), code.as_code_str());
    }

    #[test]
    fn serde_uses_screaming_snake_case() {
        let json = serde_json::to_string(&ToolCode::EditRequiresPriorRead).unwrap();
        assert_eq!(json, "\"EDIT_REQUIRES_PRIOR_READ\"");
        let back: ToolCode = serde_json::from_str("\"AMBIGUOUS_MATCH\"").unwrap();
        assert_eq!(back, ToolCode::AmbiguousMatch);
    }
}
