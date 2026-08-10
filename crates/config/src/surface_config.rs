//! Secondary on-disk config tables: hook sinks, skills, tools,
//! snapshots, network policy, and LSP diagnostics injection.
//!
//! Extracted from `lib.rs` during the config crate split
//! (CODE_STRUCTURE_ANALYSIS.md §3.3).
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// On-disk schema for the `[hook_sinks]` table.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HookSinksToml {
    /// Unix domain socket path used by the app-server event sink.
    ///
    /// When unset, no Unix socket sink is registered. There is deliberately no
    /// shared `/tmp` default because socket ownership should be explicit.
    #[serde(default)]
    pub unix_socket_path: Option<PathBuf>,
}

/// On-disk schema for the `[skills]` table (#140). See `config.example.toml`
/// for documentation.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SkillsToml {
    /// Curated registry index URL. When unset, the TUI falls back to the
    /// bundled default (community-curated GitHub raw).
    #[serde(default)]
    pub registry_url: Option<String>,
    /// Per-skill maximum *uncompressed* size in bytes. When unset, the TUI
    /// uses 5 MiB.
    #[serde(default)]
    pub max_install_size_bytes: Option<u64>,
}

/// On-disk schema for the `[tools]` table (#2076).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolsToml {
    /// Native tool names to keep loaded outside the default core catalog.
    #[serde(default)]
    pub always_load: Vec<String>,
}

/// On-disk schema for the `[snapshots]` table (#137). See
/// `config.example.toml` for documentation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotsToml {
    #[serde(default = "default_snapshots_enabled")]
    pub enabled: bool,
    #[serde(default = "default_snapshot_max_age_days")]
    pub max_age_days: u64,
}

fn default_snapshots_enabled() -> bool {
    true
}

fn default_snapshot_max_age_days() -> u64 {
    7
}

impl Default for SnapshotsToml {
    fn default() -> Self {
        Self {
            enabled: default_snapshots_enabled(),
            max_age_days: default_snapshot_max_age_days(),
        }
    }
}

/// On-disk schema for the `[network]` table (#135). See `config.example.toml`
/// for documentation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkPolicyToml {
    /// Decision for hosts that are not in `allow` or `deny`. One of
    /// `"allow" | "deny" | "prompt"`. Defaults to `"prompt"`.
    #[serde(default = "default_network_decision")]
    pub default: String,
    /// Hosts that are always allowed. Subdomain rules: a leading dot
    /// (`.example.com`) matches subdomains but not the apex.
    #[serde(default)]
    pub allow: Vec<String>,
    /// Hosts that are always denied. Deny entries win over allow entries.
    #[serde(default)]
    pub deny: Vec<String>,
    /// Hostnames whose DNS may resolve to fake-IP/private proxy ranges in an
    /// explicitly trusted proxy setup. Literal IP URLs remain blocked.
    #[serde(default)]
    pub proxy: Vec<String>,
    /// Whether to record one audit-log line per outbound network call.
    #[serde(default = "default_network_audit")]
    pub audit: bool,
}

fn default_network_decision() -> String {
    "prompt".to_string()
}

fn default_network_audit() -> bool {
    true
}

impl Default for NetworkPolicyToml {
    fn default() -> Self {
        Self {
            default: default_network_decision(),
            allow: Vec::new(),
            deny: Vec::new(),
            proxy: Vec::new(),
            audit: default_network_audit(),
        }
    }
}

/// On-disk schema for the `[cost_budget]` table (#620). See
/// `config.example.toml` for documentation.
///
/// Lets a user cap spend per session and per calendar day. When the accrued
/// cost crosses `warn_percent` of a limit a soft warning alert is emitted;
/// crossing the limit itself emits a hard alert. Blocking on exceed is
/// intentionally out of scope (high blast radius) — only alerts are produced.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CostBudgetToml {
    /// Master switch. When `false` or absent, no cost budget alerts fire.
    #[serde(default)]
    pub enabled: bool,
    /// Per-session cost ceiling in USD. The session high-water cost
    /// (`session_cost + subagent_cost`, which never decreases) is compared
    /// against this value. `0.0` / unset means "no session limit".
    #[serde(default)]
    pub session_limit_usd: f64,
    /// Per-calendar-day cost ceiling in USD, accrued across all sessions that
    /// day. `0.0` / unset means "no daily limit".
    #[serde(default)]
    pub daily_limit_usd: f64,
    /// Fraction (0.0–1.0) of a limit at which a soft warning alert fires,
    /// before the hard limit is reached. Defaults to `0.8`.
    #[serde(default)]
    pub warn_percent: f64,
}

impl CostBudgetToml {
    /// Whether this config actually defines any active budget.
    pub fn has_limit(&self) -> bool {
        self.enabled && (self.session_limit_usd > 0.0 || self.daily_limit_usd > 0.0)
    }
}

/// On-disk schema for the `[lsp]` table (#136). See `config.example.toml`
/// for documentation. All fields are optional so the TUI runtime can fall
/// back to its own defaults when keys are absent.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LspConfigToml {
    /// Master switch.
    pub enabled: Option<bool>,
    /// Maximum time to wait for diagnostics after an edit, in milliseconds.
    pub poll_after_edit_ms: Option<u64>,
    /// Cap on diagnostics surfaced per file.
    pub max_diagnostics_per_file: Option<usize>,
    /// When `true`, warnings (severity 2) are surfaced in addition to errors.
    pub include_warnings: Option<bool>,
    /// Optional override for the `language -> [cmd, ...args]` table.
    pub servers: Option<BTreeMap<String, Vec<String>>>,
}
