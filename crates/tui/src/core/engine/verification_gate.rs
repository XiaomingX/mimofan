//! Verification-stop gate.
//!
//! Tracks whether the model verified code changes it made during a turn. If a
//! file-modifying tool (write/edit/apply_patch) succeeded but no verification
//! evidence (test/build/verifier run) appeared before the turn ended, we inject
//! a single bounded nudge reminding the model to verify before declaring done.
//!
//! This is the *behavioral* counterpart to the goal-level `GoalGate`: the latter
//! gates on objective completion; this gate gates on "did you check your work".
//! It never blocks — it only nudges, at most once per turn.

/// Tools whose success counts as "the model edited code".
pub const CODE_EDIT_TOOLS: &[&str] = &[
    "write", "edit", "multi_edit", "notebook_edit", "write_file", "edit_file", "apply_patch",
];

/// Tools/commands whose success counts as "the model verified the change".
///
/// These are the harness-native verification surfaces plus the `bash`/shell
/// escape hatch (so `cargo test`, `npm test`, `make check`, etc. all count).
pub const VERIFICATION_TOOLS: &[&str] = &["test", "build", "verifier", "run_verifiers", "bash", "shell"];

/// Substrings that, when present in a `bash`/`shell` invocation, indicate a
/// verification command rather than an arbitrary side-effecting script.
pub const VERIFICATION_CMD_HINTS: &[&str] = &[
    "cargo test",
    "cargo build",
    "cargo check",
    "npm test",
    "npm run build",
    "pytest",
    "make test",
    "make check",
    "go test",
    "cargo nextest",
];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VerificationGate {
    code_edited: bool,
    verified: bool,
    nudged: bool,
}

impl VerificationGate {
    /// Fresh gate for a new turn.
    pub fn new() -> Self {
        Self::default()
    }

    /// Reset all tracked state (call at the start of each turn).
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Whether `name` is a code-editing tool.
    pub fn is_code_edit_tool(name: &str) -> bool {
        CODE_EDIT_TOOLS.contains(&name)
    }

    /// Whether `name` is a native verification tool, or (for `bash`/`shell`) the
    /// invocation `input` looks like a verification command.
    pub fn is_verification_tool(name: &str, input: &str) -> bool {
        if VERIFICATION_TOOLS.contains(&name) {
            if name == "bash" || name == "shell" {
                return VERIFICATION_CMD_HINTS
                    .iter()
                    .any(|hint| input.contains(hint));
            }
            return true;
        }
        false
    }

    /// Record that a code-edit tool succeeded.
    pub fn record_code_edit(&mut self) {
        self.code_edited = true;
    }

    /// Record that a verification tool succeeded.
    pub fn record_verification(&mut self) {
        self.verified = true;
    }

    /// Observe a tool outcome: updates internal state from a successful tool
    /// call. Returns nothing; query `should_nudge` separately at turn end.
    pub fn observe(&mut self, tool_name: &str, input: &str, success: bool) {
        if !success {
            return;
        }
        if Self::is_code_edit_tool(tool_name) {
            self.record_code_edit();
        } else if Self::is_verification_tool(tool_name, input) {
            self.record_verification();
        }
    }

    /// True iff code was edited, never verified, and we have not nudged yet.
    /// Consuming: flips `nudged` so the nudge fires at most once per turn.
    pub fn should_nudge(&mut self) -> bool {
        if self.code_edited && !self.verified && !self.nudged {
            self.nudged = true;
            true
        } else {
            false
        }
    }

    pub fn nudge_text() -> &'static str {
        "[verification reminder] You edited code this turn but did not run any \
test/build/verifier. Before finishing, verify your changes (e.g. `cargo test`, \
`cargo build`, or a project-specific check) so the change is confirmed to work."
    }

    /// Snapshot accessors for tests/inspection.
    pub fn code_edited(&self) -> bool {
        self.code_edited
    }
    pub fn verified(&self) -> bool {
        self.verified
    }
    pub fn nudged(&self) -> bool {
        self.nudged
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_then_verify_does_not_nudge() {
        let mut g = VerificationGate::new();
        g.observe("edit", "", true);
        g.observe("bash", "cargo test", true);
        assert!(!g.should_nudge(), "verified edits must not nudge");
    }

    #[test]
    fn edit_without_verify_nudges_once() {
        let mut g = VerificationGate::new();
        g.observe("write_file", "", true);
        assert!(g.should_nudge(), "unverified edit should nudge");
        // Second call must not re-nudge within the same turn.
        assert!(!g.should_nudge(), "nudge must be bounded to once per turn");
        assert!(g.nudged());
    }

    #[test]
    fn failed_edit_does_not_count() {
        let mut g = VerificationGate::new();
        g.observe("edit", "", false);
        assert!(!g.should_nudge(), "failed edits are not work to verify");
    }

    #[test]
    fn bash_without_test_hint_is_not_verification() {
        let mut g = VerificationGate::new();
        g.observe("edit", "", true);
        g.observe("bash", "rm -rf target", true);
        assert!(g.should_nudge(), "non-test bash must not satisfy the gate");
    }

    #[test]
    fn reset_clears_state() {
        let mut g = VerificationGate::new();
        g.observe("edit", "", true);
        assert!(g.should_nudge());
        g.reset();
        assert!(!g.code_edited());
        assert!(!g.verified());
        assert!(!g.nudged());
    }

    #[test]
    fn verification_tool_recognition() {
        assert!(VerificationGate::is_verification_tool("test", ""));
        assert!(VerificationGate::is_verification_tool("build", ""));
        assert!(VerificationGate::is_verification_tool("verifier", ""));
        assert!(VerificationGate::is_verification_tool("bash", "cargo nextest"));
        assert!(!VerificationGate::is_verification_tool("bash", "echo hi"));
        assert!(!VerificationGate::is_verification_tool("edit", ""));
    }

    #[test]
    fn code_edit_tool_recognition() {
        assert!(VerificationGate::is_code_edit_tool("edit"));
        assert!(VerificationGate::is_code_edit_tool("write_file"));
        assert!(VerificationGate::is_code_edit_tool("apply_patch"));
        assert!(!VerificationGate::is_code_edit_tool("bash"));
    }
}
