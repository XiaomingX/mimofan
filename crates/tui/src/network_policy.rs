// Several public helpers in this module are exposed for future slash-command
// wiring (`/network allow <host>`, `/network deny <host>`) and for the
// approval-modal hook that v0.7.x adds incrementally. Dead-code warnings
// would otherwise be noisy until those call sites land.
#![allow(dead_code)]
// Audit-write failure must route through `tracing::*`, not raw stderr —
// see `runtime_log` for the scroll-demon rationale.
#![deny(clippy::print_stdout)]
#![deny(clippy::print_stderr)]

//! Per-domain network policy for outbound network calls (#135).
//!
//! Three small pieces:
//!
//! 1. [`Decision`] — `Allow | Deny | Prompt`.
//! 2. [`NetworkPolicy`] — a list of allow/deny hostnames + a default decision,
//!    with **deny-wins precedence**: a host that matches an entry in `deny`
//!    is denied even if it also matches `allow`.
//! 3. [`NetworkAuditor`] — appends one plaintext line per outbound call to
//!    `~/.mimofan/audit.log` in the format described below.
//!
//! In addition, [`NetworkSessionCache`] holds in-process "approve once for
//! this session" state for the `Prompt` flow, and [`NetworkDenied`] is the
//! structured error surfaced to callers when a host is blocked.
//!
//! # Host-matching rules
//!
//! * **Exact match** — an entry like `api.deepseek.com` matches only the host
//!   `api.deepseek.com` (case-insensitive).
//! * **Subdomain match** — an entry that **starts with a leading dot**, e.g.
//!   `.example.com`, matches any subdomain (`api.example.com`, `a.b.example.com`)
//!   but **not** the apex `example.com`. To match both, list both.
//!
//! Matching is case-insensitive and trims a single trailing dot from the host
//! (so `example.com.` and `example.com` are equivalent).
//!
//! # Audit-log format
//!
//! ```text
//! <RFC3339-timestamp> network <host> <tool> <Allow|Deny|Prompt-Approved|Prompt-Denied|TrustedProxyFakeIp-Allow>
//! ```
//!
//! Plaintext, one line per call, appended to `<audit_path>` (defaults to
//! `~/.mimofan/audit.log`). Best-effort: write failures are logged but do
//! not block the call.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// What the policy decided about an outbound network call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Allow the call without prompting.
    Allow,
    /// Deny the call. Surfaced to callers as [`NetworkDenied`].
    Deny,
    /// Defer to the user via an approval prompt.
    Prompt,
}

impl Decision {
    /// String form used in audit-log lines.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "Allow",
            Self::Deny => "Deny",
            Self::Prompt => "Prompt",
        }
    }

    /// Parse a decision from a TOML string. Unknown values fall back to
    /// `Prompt` so a typo never silently disables the policy.
    #[must_use]
    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "allow" => Self::Allow,
            "deny" | "block" => Self::Deny,
            _ => Self::Prompt,
        }
    }
}

/// Per-domain allow/deny list with a default fallback.
///
/// See the module docs for [host-matching rules](self#host-matching-rules)
/// and [deny-wins precedence](self#deny-wins-precedence).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkPolicy {
    /// Decision for hosts that match neither `allow` nor `deny`.
    #[serde(default = "default_decision")]
    pub default: DecisionToml,
    /// Hosts that should be allowed without prompting.
    #[serde(default)]
    pub allow: Vec<String>,
    /// Hosts that should always be denied.
    #[serde(default)]
    pub deny: Vec<String>,
    /// Hostnames whose DNS may resolve to fake-IP/private proxy ranges in an
    /// explicitly trusted proxy setup. This does not affect literal IP URLs.
    #[serde(default)]
    pub proxy: Vec<String>,
    /// Whether to record one audit-log line per network call. Defaults to true.
    #[serde(default = "default_audit")]
    pub audit: bool,
}

fn default_decision() -> DecisionToml {
    DecisionToml::Prompt
}

fn default_audit() -> bool {
    true
}

impl Default for NetworkPolicy {
    fn default() -> Self {
        Self {
            default: DecisionToml::Prompt,
            allow: Vec::new(),
            deny: Vec::new(),
            proxy: Vec::new(),
            audit: true,
        }
    }
}

/// Wire-format wrapper for [`Decision`] used in serde-derived TOML/JSON. The
/// runtime API exposes [`Decision`] directly; this type only exists so
/// `default = "prompt"` round-trips cleanly through TOML.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DecisionToml {
    Allow,
    Deny,
    Prompt,
}

impl From<DecisionToml> for Decision {
    fn from(value: DecisionToml) -> Self {
        match value {
            DecisionToml::Allow => Self::Allow,
            DecisionToml::Deny => Self::Deny,
            DecisionToml::Prompt => Self::Prompt,
        }
    }
}

impl From<Decision> for DecisionToml {
    fn from(value: Decision) -> Self {
        match value {
            Decision::Allow => Self::Allow,
            Decision::Deny => Self::Deny,
            Decision::Prompt => Self::Prompt,
        }
    }
}

impl NetworkPolicy {
    /// Decide what to do for a single outbound call to `host`.
    ///
    /// **Deny-wins precedence**: if `host` matches any entry in `deny`, the
    /// answer is [`Decision::Deny`] regardless of `allow`. This makes deny
    /// lists safe to combine with broad allow rules.
    #[must_use]
    pub fn decide(&self, host: &str) -> Decision {
        let normalized = normalize_host(host);
        if normalized.is_empty() {
            // We don't pretend we can audit a malformed host; treat it as the
            // default (prompt or deny).
            return self.default.into();
        }
        if self
            .deny
            .iter()
            .any(|entry| host_matches(entry, &normalized))
        {
            return Decision::Deny;
        }
        if self
            .allow
            .iter()
            .any(|entry| host_matches(entry, &normalized))
        {
            return Decision::Allow;
        }
        self.default.into()
    }

    /// Append `host` to the allow list (de-duplicated, case-insensitive).
    /// Used by the prompt flow when the user picks "always for this host".
    pub fn add_allow(&mut self, host: &str) {
        let normalized = normalize_host(host);
        if normalized.is_empty() {
            return;
        }
        if !self
            .allow
            .iter()
            .any(|existing| normalize_host(existing) == normalized)
        {
            self.allow.push(normalized);
        }
    }

    /// Whether audit logging is enabled.
    #[must_use]
    pub fn audit_enabled(&self) -> bool {
        self.audit
    }

    /// Whether `host` is explicitly trusted to resolve through a local
    /// fake-IP proxy. Deny entries still win over this list.
    #[must_use]
    pub fn trusts_proxy_fakeip_host(&self, host: &str) -> bool {
        let normalized = normalize_host(host);
        if normalized.is_empty() {
            return false;
        }
        if self
            .deny
            .iter()
            .any(|entry| host_matches(entry, &normalized))
        {
            return false;
        }
        self.proxy
            .iter()
            .any(|entry| host_matches(entry, &normalized))
    }
}

/// Normalize a host for matching: lowercase, trim whitespace, strip a single
/// trailing dot (FQDN form), and strip a leading `*.` or `.` for entries that
/// are written that way in config (we treat both as subdomain wildcards on
/// the *match* side, but on input normalization we keep the leading dot so
/// `host_matches` can detect the wildcard intent).
fn normalize_host(host: &str) -> String {
    let trimmed = host.trim().trim_end_matches('.').to_ascii_lowercase();
    if let Some(rest) = trimmed.strip_prefix("*.") {
        format!(".{rest}")
    } else {
        trimmed
    }
}

/// Match a single allow/deny entry against an already-normalized host.
fn host_matches(entry: &str, normalized_host: &str) -> bool {
    let entry_norm = normalize_host(entry);
    if let Some(suffix) = entry_norm.strip_prefix('.') {
        // Wildcard subdomain rule. Match any host ending in `.suffix`, but
        // *not* the bare `suffix` itself (per spec).
        if suffix.is_empty() {
            return false;
        }
        normalized_host.ends_with(&format!(".{suffix}"))
    } else {
        entry_norm == normalized_host
    }
}

/// Parse an IPv4 CIDR string such as `"198.18.0.0/15"` into `(base, prefix)`.
/// Returns `None` for malformed input or a prefix length above 32.
fn parse_ipv4_cidr(cidr: &str) -> Option<(Ipv4Addr, u8)> {
    let (addr, prefix) = cidr.split_once('/')?;
    let base: Ipv4Addr = addr.trim().parse().ok()?;
    let prefix: u8 = prefix.trim().parse().ok()?;
    if prefix > 32 {
        return None;
    }
    Some((base, prefix))
}

/// Whether `ip` is contained in the `base/prefix` IPv4 CIDR block.
fn ipv4_in_cidr(ip: Ipv4Addr, base: Ipv4Addr, prefix: u8) -> bool {
    if prefix == 0 {
        return true;
    }
    let mask: u32 = u32::MAX << (32 - prefix);
    (u32::from(ip) & mask) == (u32::from(base) & mask)
}

/// Best-effort writer for the network audit log.
#[derive(Debug, Clone)]
pub struct NetworkAuditor {
    path: PathBuf,
    enabled: bool,
}

impl NetworkAuditor {
    /// New auditor that writes to `path`. `enabled = false` turns it into a no-op.
    #[must_use]
    pub fn new(path: PathBuf, enabled: bool) -> Self {
        Self { path, enabled }
    }

    /// Auditor pointing at `~/.mimofan/audit.log`. Returns `None` if the
    /// home directory can't be resolved.
    #[must_use]
    pub fn default_path(enabled: bool) -> Option<Self> {
        let home = dirs::home_dir()?;
        Some(Self::new(home.join(".mimofan").join("audit.log"), enabled))
    }

    /// Append one line. Best-effort: errors are logged via `eprintln!` but
    /// never bubble back to the caller.
    pub fn record(&self, host: &str, tool: &str, decision_label: &str) {
        if !self.enabled {
            return;
        }
        if let Err(err) = self.try_record(host, tool, decision_label) {
            // Routed through tracing so it lands in
            // `~/.mimofan/logs/tui-YYYY-MM-DD.log` rather than the
            // alt-screen — see `runtime_log` for the scroll-demon
            // rationale.
            tracing::warn!(target: "network_policy", ?err, host, tool, "network audit write failed");
        }
    }

    fn try_record(&self, host: &str, tool: &str, decision_label: &str) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        writeln!(
            file,
            "{ts} network {host} {tool} {decision}",
            ts = Utc::now().to_rfc3339(),
            host = sanitize_field(host),
            tool = sanitize_field(tool),
            decision = decision_label,
        )
    }

    /// Path the auditor would write to. Mostly useful for tests.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Replace whitespace in a token so the line stays parseable.
fn sanitize_field(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_whitespace() { '_' } else { c })
        .collect()
}

/// In-process cache of "approve once for this session" decisions. Keyed by
/// normalized host. Thread-safe.
#[derive(Debug, Default, Clone)]
pub struct NetworkSessionCache {
    inner: Arc<Mutex<NetworkSessionCacheInner>>,
}

#[derive(Debug, Default)]
struct NetworkSessionCacheInner {
    approved: std::collections::HashSet<String>,
    denied: std::collections::HashSet<String>,
}

impl NetworkSessionCache {
    /// New empty cache.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `true` if the host was previously approved this session.
    #[must_use]
    pub fn is_approved(&self, host: &str) -> bool {
        let normalized = normalize_host(host);
        self.inner
            .lock()
            .map(|guard| guard.approved.contains(&normalized))
            .unwrap_or(false)
    }

    /// `true` if the host was previously denied this session.
    #[must_use]
    pub fn is_denied(&self, host: &str) -> bool {
        let normalized = normalize_host(host);
        self.inner
            .lock()
            .map(|guard| guard.denied.contains(&normalized))
            .unwrap_or(false)
    }

    /// Mark the host as approved for the rest of this session.
    pub fn approve(&self, host: &str) {
        let normalized = normalize_host(host);
        if let Ok(mut guard) = self.inner.lock() {
            guard.denied.remove(&normalized);
            guard.approved.insert(normalized);
        }
    }

    /// Mark the host as denied for the rest of this session.
    pub fn deny(&self, host: &str) {
        let normalized = normalize_host(host);
        if let Ok(mut guard) = self.inner.lock() {
            guard.approved.remove(&normalized);
            guard.denied.insert(normalized);
        }
    }
}

/// Structured error surfaced to callers when an outbound call is blocked.
#[derive(Debug, Clone, Error)]
#[error("network call to '{0}' blocked by network policy")]
pub struct NetworkDenied(pub String);

impl NetworkDenied {
    /// The host that was denied.
    #[must_use]
    pub fn host(&self) -> &str {
        &self.0
    }
}

/// Glue type that bundles a [`NetworkPolicy`] with a session cache and an
/// auditor. Tools call [`NetworkPolicyDecider::evaluate`] before any HTTP
/// transport is constructed; the result decides whether to proceed, deny,
/// or prompt the user.
#[derive(Debug, Clone)]
pub struct NetworkPolicyDecider {
    policy: NetworkPolicy,
    cache: NetworkSessionCache,
    auditor: Option<NetworkAuditor>,
    /// IPv4 CIDR ranges that are treated as benign fake-IP placeholders (e.g.
    /// a transparent-proxy / TUN setup running in `fake-ip` mode, where DNS
    /// resolves every hostname into a reserved range like `198.18.0.0/15`).
    /// A resolved IP inside one of these ranges bypasses the restricted-IP SSRF
    /// block; real private/loopback/link-local/metadata IPs are unaffected.
    trusted_fakeip_cidrs: Vec<(Ipv4Addr, u8)>,
}

impl NetworkPolicyDecider {
    /// Build a decider from a policy. The session cache starts empty.
    #[must_use]
    pub fn new(policy: NetworkPolicy, auditor: Option<NetworkAuditor>) -> Self {
        Self {
            policy,
            cache: NetworkSessionCache::new(),
            auditor,
            trusted_fakeip_cidrs: Vec::new(),
        }
    }

    /// Register IPv4 CIDR ranges to treat as benign fake-IP placeholders.
    /// Invalid CIDR strings are skipped. See [`Self::is_trusted_fakeip_addr`].
    #[must_use]
    pub fn with_trusted_fakeip_cidrs(mut self, cidrs: &[&str]) -> Self {
        for cidr in cidrs {
            if let Some(parsed) = parse_ipv4_cidr(cidr) {
                self.trusted_fakeip_cidrs.push(parsed);
            }
        }
        self
    }

    /// Whether `ip` falls inside a configured fake-IP placeholder range.
    ///
    /// In `fake-ip` proxy/TUN setups the local resolver maps every hostname to
    /// a reserved range (commonly `198.18.0.0/15`), so the DNS-resolution SSRF
    /// check would otherwise reject every request. This narrowly trusts only
    /// those placeholder addresses — real private/loopback/link-local/cloud-
    /// metadata IPs are *not* matched and stay blocked.
    #[must_use]
    pub fn is_trusted_fakeip_addr(&self, ip: &IpAddr) -> bool {
        match ip {
            IpAddr::V4(v4) => self
                .trusted_fakeip_cidrs
                .iter()
                .any(|(base, prefix)| ipv4_in_cidr(*v4, *base, *prefix)),
            // fake-ip placeholders are IPv4-only in practice.
            IpAddr::V6(_) => false,
        }
    }

    /// Convenience: build a decider with default audit logging at
    /// `~/.mimofan/audit.log`, if `policy.audit` is true.
    #[must_use]
    pub fn with_default_audit(policy: NetworkPolicy) -> Self {
        let audit_enabled = policy.audit_enabled();
        let auditor = if audit_enabled {
            NetworkAuditor::default_path(true)
        } else {
            None
        };
        Self::new(policy, auditor)
    }

    /// Inspect the policy.
    #[must_use]
    pub fn policy(&self) -> &NetworkPolicy {
        &self.policy
    }

    /// Inspect the session cache.
    #[must_use]
    pub fn cache(&self) -> &NetworkSessionCache {
        &self.cache
    }

    /// Decide for `host`, consulting the session cache first.
    ///
    /// Audit logging happens **only** for terminal decisions (Allow / Deny).
    /// `Prompt` is intentionally not logged here — the caller is responsible
    /// for recording the user's eventual answer with `record_prompt_outcome`.
    #[must_use]
    pub fn evaluate(&self, host: &str, tool: &str) -> Decision {
        let normalized = normalize_host(host);
        if normalized.is_empty() {
            return self.policy.default.into();
        }
        if self.cache.is_denied(&normalized) {
            self.audit_record(&normalized, tool, "Deny");
            return Decision::Deny;
        }
        if self.cache.is_approved(&normalized) {
            self.audit_record(&normalized, tool, "Allow");
            return Decision::Allow;
        }
        let decision = self.policy.decide(&normalized);
        match decision {
            Decision::Allow => self.audit_record(&normalized, tool, "Allow"),
            Decision::Deny => self.audit_record(&normalized, tool, "Deny"),
            Decision::Prompt => {}
        }
        decision
    }

    /// Approve `host` for the rest of the session (one-shot). Audit log gets
    /// `Prompt-Approved`.
    pub fn approve_session(&self, host: &str, tool: &str) {
        self.cache.approve(host);
        self.audit_record(host, tool, "Prompt-Approved");
    }

    /// Deny `host` for the rest of the session. Audit log gets `Prompt-Denied`.
    pub fn deny_session(&self, host: &str, tool: &str) {
        self.cache.deny(host);
        self.audit_record(host, tool, "Prompt-Denied");
    }

    /// Persist `host` into the policy's allow list (so it survives the session)
    /// **and** approve it in-session. Returns the updated policy so callers can
    /// write it back to disk.
    pub fn approve_persistent(&mut self, host: &str, tool: &str) -> &NetworkPolicy {
        self.policy.add_allow(host);
        self.cache.approve(host);
        self.audit_record(host, tool, "Prompt-Approved");
        &self.policy
    }

    /// Whether this host is explicitly configured for trusted proxy fake-IP
    /// DNS handling.
    #[must_use]
    pub fn trusts_proxy_fakeip_host(&self, host: &str) -> bool {
        self.policy.trusts_proxy_fakeip_host(host)
    }

    /// Record that a restricted DNS result was allowed because the host is in
    /// the trusted proxy fake-IP list.
    pub fn record_trusted_proxy_fakeip_allow(&self, host: &str, tool: &str) {
        self.audit_record(host, tool, "TrustedProxyFakeIp-Allow");
    }

    fn audit_record(&self, host: &str, tool: &str, label: &str) {
        if let Some(auditor) = self.auditor.as_ref() {
            auditor.record(host, tool, label);
        }
    }
}

/// Extract the host portion of a URL, lowercased. Returns `None` if the URL
/// can't be parsed or has no host.
#[must_use]
pub fn host_from_url(url: &str) -> Option<String> {
    let parsed = reqwest::Url::parse(url.trim()).ok()?;
    parsed.host_str().map(str::to_ascii_lowercase)
}

#[cfg(test)]
mod tests {}
