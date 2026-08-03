// Tests relocated from src/execpolicy/matcher.rs (issue #547 Phase 3).

use mimofan::execpolicy::matcher::*;

#[test]
fn deny_bypass_closed_by_canonical_form() {
    // `rm *` must match the path/wrapper forms via the canonical variant.
    assert!(pattern_matches("rm *", "/bin/rm -rf /"));
    assert!(pattern_matches("rm *", "sudo rm -rf /"));
    // Bare `rm` (no wildcard) still requires exact match of the executable.
    assert!(!pattern_matches("rm", "rm -rf /"));
}
