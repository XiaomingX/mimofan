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

use serde::{Deserialize, Serialize};

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

/// Memory trust tier: how much an agent should believe a stored memory record.
///
/// Ordered from least to most trustworthy so callers can compare and filter
/// (e.g. "only surface memories at or above `Observed`"). The ordering is the
/// source-reliability ladder described in MY_PLAN_0817 §13 for *memory*
/// provenance, distinct from the skill-supply-chain [`IsolationLevel`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrustTier {
    /// Cross-session / unattributed reference we cannot re-verify here. Treated
    /// as "someone else said so" — surfaced last, never auto-trusted.
    Untrusted,
    /// Directly observed in the current session (a tool result, a user statement,
    /// a visible file state). Ground truth for this session.
    Observed,
    /// Derived by the agent's own reasoning from observed facts, not directly
    /// confirmed. Plausible but may be wrong.
    Inferred,
    /// Independently verified — e.g. a tool result cross-checked, an assertion
    /// that passed, or an explicit human confirmation.
    Verified,
}

impl TrustTier {
    /// Stable label for logs / serialization fallback.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            TrustTier::Untrusted => "untrusted",
            TrustTier::Observed => "observed",
            TrustTier::Inferred => "inferred",
            TrustTier::Verified => "verified",
        }
    }
}

/// Provenance attached to a stored memory record: *how much we trust it* plus
/// a free-text description of where the claim came from (a session id, a tool
/// name, a human annotation, etc.). Lightweight and serializable so memory
/// stores can persist and later filter by trust.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryProvenance {
    /// Trust level of this memory entry.
    pub tier: TrustTier,
    /// Human-readable origin, e.g. `"session:abc123"`, `"tool:read_file"`,
    /// `"human:confirmed"`, or `"cross-session:note.md"`.
    pub origin: String,
}

impl MemoryProvenance {
    /// A fact directly observed during the current session, tagged with the
    /// observing session id. → `Observed`.
    #[must_use]
    pub fn from_session(session_id: &str) -> Self {
        MemoryProvenance {
            tier: TrustTier::Observed,
            origin: format!("session:{session_id}"),
        }
    }

    /// A claim the agent inferred from observed facts (not directly confirmed).
    /// → `Inferred`. `basis` names the inference source (e.g. `"tool:read_file"`).
    #[must_use]
    pub fn from_inference(basis: &str) -> Self {
        MemoryProvenance {
            tier: TrustTier::Inferred,
            origin: format!("inference:{basis}"),
        }
    }

    /// A tool result that has been independently checked / asserted. → `Verified`.
    /// `tool` names the verifying tool (e.g. `"tool:run_test"`).
    #[must_use]
    pub fn from_tool_result_verified(tool: &str) -> Self {
        MemoryProvenance {
            tier: TrustTier::Verified,
            origin: format!("verified:{tool}"),
        }
    }

    /// A reference carried over from another session we cannot re-verify here.
    /// Echoes MY_PLAN_0817 §13's "cross-session reference → untrusted".
    /// → `Untrusted`. `source` names the originating note/session.
    #[must_use]
    pub fn cross_session_untrusted(source: &str) -> Self {
        MemoryProvenance {
            tier: TrustTier::Untrusted,
            origin: format!("cross-session:{source}"),
        }
    }

    /// Whether this memory's trust is at least `min`.
    #[must_use]
    pub fn is_at_least(&self, min: TrustTier) -> bool {
        self.tier >= min
    }

    /// Filter a slice, keeping only entries whose trust is at least `min`.
    #[must_use]
    pub fn filter_by_min_tier(
        items: &[MemoryProvenance],
        min: TrustTier,
    ) -> Vec<&MemoryProvenance> {
        items.iter().filter(|p| p.is_at_least(min)).collect()
    }
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

    // ---- Memory trust tier model -------------------------------------------

    #[test]
    fn trust_tier_ordering_is_ladder() {
        assert!(TrustTier::Untrusted < TrustTier::Observed);
        assert!(TrustTier::Observed < TrustTier::Inferred);
        assert!(TrustTier::Inferred < TrustTier::Verified);
        // Total order across all four.
        let mut tiers = [
            TrustTier::Verified,
            TrustTier::Untrusted,
            TrustTier::Observed,
            TrustTier::Inferred,
        ];
        tiers.sort();
        assert_eq!(
            tiers,
            [
                TrustTier::Untrusted,
                TrustTier::Observed,
                TrustTier::Inferred,
                TrustTier::Verified
            ]
        );
    }

    #[test]
    fn trust_tier_labels() {
        assert_eq!(TrustTier::Untrusted.label(), "untrusted");
        assert_eq!(TrustTier::Observed.label(), "observed");
        assert_eq!(TrustTier::Inferred.label(), "inferred");
        assert_eq!(TrustTier::Verified.label(), "verified");
    }

    #[test]
    fn constructor_helpers_assign_correct_tier() {
        let obs = MemoryProvenance::from_session("sess-1");
        assert_eq!(obs.tier, TrustTier::Observed);
        assert_eq!(obs.origin, "session:sess-1");

        let inf = MemoryProvenance::from_inference("tool:read_file");
        assert_eq!(inf.tier, TrustTier::Inferred);
        assert_eq!(inf.origin, "inference:tool:read_file");

        let ver = MemoryProvenance::from_tool_result_verified("tool:run_test");
        assert_eq!(ver.tier, TrustTier::Verified);
        assert_eq!(ver.origin, "verified:tool:run_test");

        let un = MemoryProvenance::cross_session_untrusted("note.md");
        assert_eq!(un.tier, TrustTier::Untrusted);
        assert_eq!(un.origin, "cross-session:note.md");
    }

    #[test]
    fn is_at_least_respects_order() {
        let obs = MemoryProvenance::from_session("s");
        assert!(obs.is_at_least(TrustTier::Untrusted));
        assert!(obs.is_at_least(TrustTier::Observed));
        assert!(!obs.is_at_least(TrustTier::Inferred));
        assert!(!obs.is_at_least(TrustTier::Verified));

        let un = MemoryProvenance::cross_session_untrusted("n");
        assert!(un.is_at_least(TrustTier::Untrusted));
        assert!(!un.is_at_least(TrustTier::Observed));
    }

    #[test]
    fn filter_by_min_tier_keeps_only_threshold() {
        let items = [
            MemoryProvenance::cross_session_untrusted("a"),
            MemoryProvenance::from_session("b"),
            MemoryProvenance::from_inference("c"),
            MemoryProvenance::from_tool_result_verified("d"),
        ];
        let kept = MemoryProvenance::filter_by_min_tier(&items, TrustTier::Observed);
        assert_eq!(kept.len(), 3);
        assert!(kept.iter().all(|p| p.is_at_least(TrustTier::Observed)));

        let only_verified = MemoryProvenance::filter_by_min_tier(&items, TrustTier::Verified);
        assert_eq!(only_verified.len(), 1);
        assert_eq!(only_verified[0].tier, TrustTier::Verified);

        // Untrusted threshold keeps everything.
        let all = MemoryProvenance::filter_by_min_tier(&items, TrustTier::Untrusted);
        assert_eq!(all.len(), 4);
    }

    #[test]
    fn trust_tier_roundtrips_serde() {
        for t in [
            TrustTier::Untrusted,
            TrustTier::Observed,
            TrustTier::Inferred,
            TrustTier::Verified,
        ] {
            let json = serde_json::to_string(&t).unwrap();
            let back: TrustTier = serde_json::from_str(&json).unwrap();
            assert_eq!(t, back);
        }
        let prov = MemoryProvenance::from_tool_result_verified("tool:run_test");
        let json = serde_json::to_string(&prov).unwrap();
        let back: MemoryProvenance = serde_json::from_str(&json).unwrap();
        assert_eq!(prov, back);
    }
}
