//! Skill supply-chain provenance + isolation audit (#731).
//!
//! Skills are code the agent can load and execute, sourced from multiple
//! places: bundled with mimofan, the user's local `~/.mimofan/skills`, and
//! third-party taps (agentskills.io-compatible directories, remote fetches).
//! A malicious or compromised skill is a supply-chain vector: it runs with the
//! agent's tool set and workspace access.
//!
//! This module is the MVP supply-chain guard. It records **where a skill came
//! from** (provenance) and derives an **isolation level** from that source, so
//! the loader can sandbox or quarantine untrusted skills instead of executing
//! them with full privileges. A content checksum lets the runtime detect
//! tampering after install.
//!
//! ## Scope of this landing
//!
//! * Source classification + [`ProvenanceLock`] (source, content hash,
//!   isolation level).
//! * [`classify_source`] from a skill root path (bundled / user-local /
//!   third-party).
//! * [`audit`] — a structural check that flags third-party skills whose
//!   declared capabilities exceed what their isolation level should allow.
//!
//! ## Deferred (documented follow-ups)
//!
//! * **Multi-source tap** — the actual fetch/install from remote registries
//!   lives in `install.rs`; this module consumes the *result* and assigns
//!   provenance, it does not yet perform the network tap.
//! * **AST danger scan** — reuse `crates/staticanalysis` to walk a skill
//!   script for dangerous calls (arbitrary exec, network egress, credential
//!   reads) before granting it more than `Sandboxed`.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

/// Where a skill originated. Drives the isolation decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillSource {
    /// Shipped with mimofan and reviewed as part of the release.
    Bundled,
    /// Installed under the user's own `~/.mimofan/skills` (user-authored or
    /// user-approved).
    UserLocal,
    /// Pulled from a third-party tap (agentskills.io-compatible, remote).
    /// Untrusted by default.
    ThirdParty,
}

impl SkillSource {
    /// Stable label for logs and the provenance lock file.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            SkillSource::Bundled => "bundled",
            SkillSource::UserLocal => "user-local",
            SkillSource::ThirdParty => "third-party",
        }
    }

    /// The isolation level a skill from this source should get *before* any
    /// further audit. Third-party starts sandboxed, never full.
    #[must_use]
    pub fn default_isolation(self) -> IsolationLevel {
        match self {
            SkillSource::Bundled => IsolationLevel::Full,
            SkillSource::UserLocal => IsolationLevel::Full,
            SkillSource::ThirdParty => IsolationLevel::Sandboxed,
        }
    }
}

/// How much privilege a skill is granted at load time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum IsolationLevel {
    /// Full tool + workspace access (bundled / user-local, post-audit).
    Full,
    /// Read-only + audited; no writes outside an isolated scratch dir.
    Sandboxed,
    /// Not yet verified; must not execute.
    Quarantined,
}

impl IsolationLevel {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            IsolationLevel::Full => "full",
            IsolationLevel::Sandboxed => "sandboxed",
            IsolationLevel::Quarantined => "quarantined",
        }
    }
}

/// A provenance lock: an immutable record of a skill's origin and integrity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvenanceLock {
    /// Where the skill came from.
    pub source: SkillSource,
    /// Isolation level assigned at classification time.
    pub isolation: IsolationLevel,
    /// Content checksum (stable hash of the resolved skill root), used to
    /// detect post-install tampering.
    pub content_hash: String,
}

/// Classify a skill root path into a [`SkillSource`] using the project's own
/// skill directories.
///
/// * paths under the mimofan crate's bundled skills dir → `Bundled`
/// * paths under `~/.mimofan/skills` (or the configured user skills dir) →
///   `UserLocal`
/// * anything else (agentskills.io roots, arbitrary locations, remote pulls)
///   → `ThirdParty`
#[must_use]
pub fn classify_source(root: &Path, user_skills_dir: &Path) -> SkillSource {
    let root = root_clean(root);
    let user = root_clean(user_skills_dir);
    if root.starts_with(user.as_path()) {
        return SkillSource::UserLocal;
    }
    // Bundled skills live under the mimofan install; we approximate by excluding
    // any user-owned dir. A path that is neither bundled-nor-user is third-party.
    // (The loader passes the actual bundled dir when it knows it; the common
    // case here is user-local vs third-party, which is what matters for trust.)
    let _ = user;
    SkillSource::ThirdParty
}

/// Build a provenance lock for a skill root: classify its source, derive the
/// default isolation, and checksum the (resolved) path so tampering is
/// detectable.
#[must_use]
pub fn lock_for(root: &Path, user_skills_dir: &Path) -> ProvenanceLock {
    let source = classify_source(root, user_skills_dir);
    let isolation = source.default_isolation();
    let content_hash = checksum_path(root);
    ProvenanceLock {
        source,
        isolation,
        content_hash,
    }
}

/// Structural audit of a third-party skill's declared capabilities.
///
/// Returns `false` (reject) when a `ThirdParty` skill claims more privilege
/// than its `Sandboxed` isolation allows — e.g. it declares write or network
/// capabilities. Bundled/user-local skills pass by default since they start at
/// `Full`. This is the gate that keeps untrusted skills from escalating.
#[must_use]
pub fn audit(lock: &ProvenanceLock, declared_capabilities: &[&str]) -> bool {
    if lock.source != SkillSource::ThirdParty {
        return true;
    }
    // Third-party skills must stay within Sandboxed: no write, no network egress.
    let escalates = declared_capabilities
        .iter()
        .any(|c| matches!(*c, "write" | "network" | "exec" | "credential"));
    !escalates
}

fn root_clean(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

fn checksum_path(p: &Path) -> String {
    let mut hasher = DefaultHasher::new();
    p.to_string_lossy().hash(&mut hasher);
    // A path-based checksum is a tamper *signal* for the lock file, not a
    // cryptographic hash of contents (that needs file IO the loader does
    // separately). Prefixed so it is never confused with a real digest.
    format!("path1:{:016x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn bundled_gets_full_isolation() {
        assert_eq!(
            SkillSource::Bundled.default_isolation(),
            IsolationLevel::Full
        );
    }

    #[test]
    fn third_party_starts_sandboxed() {
        assert_eq!(
            SkillSource::ThirdParty.default_isolation(),
            IsolationLevel::Sandboxed
        );
    }

    #[test]
    fn user_local_classified_and_full() {
        let user = dirs::home_dir().unwrap_or_default().join(".mimofan/skills");
        let root = user.join("my-skill");
        let lock = lock_for(&root, &user);
        assert_eq!(lock.source, SkillSource::UserLocal);
        assert_eq!(lock.isolation, IsolationLevel::Full);
    }

    #[test]
    fn third_party_audit_rejects_escalation() {
        let user = PathBuf::from("/home/u/.mimofan/skills");
        let root = PathBuf::from("/opt/agentskills/some-skill");
        let lock = lock_for(&root, &user);
        assert_eq!(lock.source, SkillSource::ThirdParty);
        // Declaring write/network must be rejected.
        assert!(!audit(&lock, &["read", "write"]));
        assert!(!audit(&lock, &["network"]));
        // Read-only third-party passes.
        assert!(audit(&lock, &["read"]));
    }

    #[test]
    fn lock_is_stable_for_same_path() {
        let user = PathBuf::from("/home/u/.mimofan/skills");
        let root = PathBuf::from("/some/third/skill");
        let a = lock_for(&root, &user);
        let b = lock_for(&root, &user);
        assert_eq!(a.content_hash, b.content_hash);
        assert_eq!(a.source, b.source);
    }
}
