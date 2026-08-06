//! Configuration loading and defaults for mimofan.

use std::collections::HashMap;
use std::fs;
#[cfg(unix)]
use std::io::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use mimofan_execpolicy::ExecPolicyEngine;
use serde::Deserialize;
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

use crate::features::{Feature, Features, FeaturesToml, is_known_feature_key};
use crate::hooks::HooksConfig;

// Unified `[limits]` configuration region: single source of truth for
// concurrency / parallelism / timeout / queue knobs. Constants and the
// `LimitsConfig` struct live here and are re-exported so `crate::config::*`
// paths resolve unchanged (#limits-region).
mod limits;
pub use limits::*;

// Sub-agent concurrency/timeout clamp resolvers re-export the relevant
// constants from `limits` so the historical `crate::config::<CONST>` paths
// keep resolving unchanged (#3311).
mod subagent_limits;
pub use subagent_limits::*;
use subagent_limits::{resolve_subagent_api_timeout_secs, resolve_subagent_heartbeat_timeout_secs};

// Provider model-name and base-URL constants live in the `models` leaf module
// and are re-exported below so every `crate::config::<CONST>` path is unchanged
// (#3311).
mod models;
pub use models::*;

// Provider types and model routing functions.
mod provider;
pub use provider::*;

// Tools configuration and status items.
mod tools;
pub use tools::*;

// Notifications and snapshot configuration.
mod notifications;
pub use notifications::*;

// Credential management: API key storage, retrieval, provider auth.
mod credential;
pub use credential::*;

// Workspace trust: config-level trust checking and saving.
mod trust;
pub(crate) use trust::*;

// Environment variable overrides for configuration values.
mod env_overrides;
pub(crate) use env_overrides::*;

// === Types ===

/// Raw retry configuration loaded from config files.
#[derive(Debug, Clone, Deserialize)]
pub struct RetryConfig {
    pub enabled: Option<bool>,
    pub max_retries: Option<u32>,
    pub initial_delay: Option<f64>,
    pub max_delay: Option<f64>,
    pub exponential_base: Option<f64>,
}

/// Deserialize `status_items` tolerantly: skip keys unknown to this build
/// instead of erroring with "unknown variant".  This lets a dev build write
/// `"balance"` (or any future item) while the stable build still parses the
/// config file successfully.
fn deser_status_items<'de, D>(deserializer: D) -> Result<Option<Vec<StatusItem>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw: Option<Vec<String>> = Option::deserialize(deserializer)?;
    Ok(raw.map(|strings| {
        strings
            .into_iter()
            .filter_map(|s| {
                StatusItem::from_key(&s).or_else(|| {
                    tracing::warn!("ignoring unknown status item {s:?} in config");
                    None
                })
            })
            .collect()
    }))
}

/// UI configuration loaded from config files.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct TuiConfig {
    pub alternate_screen: Option<String>,
    pub mouse_capture: Option<bool>,
    /// Timeout for startup terminal mode/probe calls in milliseconds.
    /// Defaults to 500ms when omitted.
    pub terminal_probe_timeout_ms: Option<u64>,
    /// Per-SSE-chunk idle timeout in seconds. Defaults to 300 seconds when
    /// omitted. `0` maps to the default; values clamp to `1..=3600`.
    pub stream_chunk_timeout_secs: Option<u64>,
    /// Ordered list of footer items the user wants visible. `None` (the field
    /// missing from `config.toml`) means "use the built-in default order"; an
    /// empty `Some(vec![])` means "show nothing in the footer".
    ///
    /// Edited interactively via `/statusline`; persisted to `tui.status_items`
    /// in `~/.mimofan/config.toml`.
    #[serde(default, deserialize_with = "deser_status_items")]
    pub status_items: Option<Vec<StatusItem>>,
    /// Emit OSC 8 hyperlink escape sequences around URLs in the transcript so
    /// supporting terminals (iTerm2, Terminal.app 13+, Ghostty, Kitty,
    /// WezTerm, Alacritty, recent gnome-terminal/konsole) make them
    /// Cmd+click-openable. Terminals without OSC 8 support render the plain
    /// label and ignore the escape. Defaults to on for macOS/Linux and off for
    /// Windows legacy consoles; set `false` to suppress everywhere (e.g. for a
    /// terminal that misrenders the sequence). OSC 8 escapes are emitted
    /// out-of-band, so buffer-column corruption is not a concern.
    pub osc8_links: Option<bool>,
    /// High-level notification trigger condition. When set, overrides the
    /// `[notifications].threshold_secs` gate from the lower-level
    /// `[notifications]` block:
    ///
    /// - `Always` — fire a turn-completion notification on every successful
    ///   turn regardless of duration. The configured `[notifications].method`
    ///   and `include_summary` flag are still respected.
    /// - `Never` — suppress all turn-completion notifications.
    /// - Unset (default) — fall back to the `[notifications]` defaults.
    pub notification_condition: Option<NotificationCondition>,
    /// When `true`, plain Up/Down on an empty composer scroll the
    /// transcript instead of recalling input history. Useful for
    /// terminals that map mouse-wheel gestures to arrow keys. Default:
    /// `true` only when mouse capture is off; otherwise `false`.
    #[serde(default)]
    pub composer_arrows_scroll: Option<bool>,
}

// Web-search `[search]` table types live in the `search` leaf module and are
// re-exported below so `crate::config::SearchProvider` (and siblings) resolve
// unchanged (#3311).
mod search;
pub use search::*;

/// Context management configuration (append-only layered context with Flash seams).
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ContextConfig {
    /// Master enable for layered context management. Default: false while
    /// v0.7.5 audits V4 prefix-cache behavior.
    #[serde(default)]
    pub enabled: Option<bool>,
    /// Include a deterministic project context pack in the stable prompt
    /// prefix. Default: true; set `[context] project_pack = false` to disable.
    #[serde(default)]
    pub project_pack: Option<bool>,
    /// Verbatim window: last N turns never summarized. Default: 16.
    #[serde(default)]
    pub verbatim_window_turns: Option<usize>,
    /// Soft seam thresholds based on the active request input estimate.
    #[serde(default)]
    pub l1_threshold: Option<usize>,
    #[serde(default)]
    pub l2_threshold: Option<usize>,
    #[serde(default)]
    pub l3_threshold: Option<usize>,
    /// Model used for seam/briefing work. Default: "deepseek-v4-flash".
    #[serde(default)]
    pub seam_model: Option<String>,
}

/// Sub-agent model overrides. Keys in `models` can be role names (`worker`,
/// `explorer`, `awaiter`) or type names (`general`, `explore`, `plan`,
/// `review`, `custom`). Per-call explicit model choices still win.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct SubagentsConfig {
    /// Top-level switch for the model-facing `agent` tool. `None` preserves
    /// the feature-flag default; `false` hides/refuses sub-agent spawning
    /// without changing the numeric queue/depth knobs.
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub default_model: Option<String>,
    #[serde(default)]
    pub worker_model: Option<String>,
    #[serde(default)]
    pub explorer_model: Option<String>,
    #[serde(default)]
    pub awaiter_model: Option<String>,
    #[serde(default)]
    pub review_model: Option<String>,
    #[serde(default)]
    pub custom_model: Option<String>,
    #[serde(default)]
    pub models: Option<HashMap<String, String>>,
    /// How many levels of nested sub-agents the interactive `agent` tool may
    /// spawn. `0` blocks the model-facing `agent` tool at this runtime depth;
    /// use `[subagents] enabled = false` for the clearer durable off switch.
    /// `1` allows one level, `2` two, and so on. When unset, defaults to
    /// [`mimofan_config::DEFAULT_SPAWN_DEPTH`]; any value is clamped to
    /// [`mimofan_config::MAX_SPAWN_DEPTH_CEILING`]. Fleet workers are
    /// governed separately by `[fleet.exec] max_spawn_depth`; both share the
    /// same default and ceiling so the limit cannot drift.
    #[serde(default)]
    pub max_depth: Option<u32>,
    /// Optional aggregate token budget shared by a root `agent` run and its
    /// descendants. When unset or 0, sub-agents keep legacy unlimited spend
    /// behavior unless an individual `agent` call supplies a per-run override.
    #[serde(default)]
    pub token_budget: Option<u64>,
    /// Per-provider overrides for sub-agent fanout and budget knobs. Keys are
    /// provider names such as `deepseek`, `zai`, `openrouter`, or `anthropic`.
    #[serde(default)]
    pub providers: Option<HashMap<String, SubagentProviderConfig>>,
}

/// Provider-specific sub-agent limit overrides.
///
/// Every field inherits from `[subagents]` when unset, so a provider profile
/// can tighten only the knobs that matter for that API's rate limits.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct SubagentProviderConfig {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub max_concurrent: Option<usize>,
    #[serde(default)]
    pub max_depth: Option<u32>,
    #[serde(default)]
    pub launch_concurrency: Option<usize>,
    #[serde(default, alias = "max_total", alias = "admission_limit")]
    pub max_admitted: Option<usize>,
    #[serde(default)]
    pub token_budget: Option<u64>,
    #[serde(default)]
    pub api_timeout_secs: Option<u64>,
    #[serde(default)]
    pub heartbeat_timeout_secs: Option<u64>,
}

/// `[auto]` table — knobs for the `--model auto` / `/model auto` router.
///
/// `cost_saving` (#1207): when `true`, the auto-mode router prefers
/// `deepseek-v4-flash` for ambiguous requests, only escalating to
/// `deepseek-v4-pro` when the task clearly benefits from deeper reasoning.
/// Default is `false` (balanced — match the existing routing voice).
#[derive(Debug, Clone, Deserialize, Default)]
pub struct AutoConfig {
    #[serde(default)]
    pub cost_saving: Option<bool>,
}

fn default_update_check_for_updates() -> bool {
    true
}

/// Startup update-check configuration (`[update]` table in config.toml).
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct UpdateConfig {
    /// When false, skip the TUI startup background update check entirely.
    #[serde(default = "default_update_check_for_updates")]
    pub check_for_updates: bool,
    /// Optional GitHub-compatible latest-release JSON endpoint.
    #[serde(default)]
    pub update_uri: Option<String>,
}

impl Default for UpdateConfig {
    fn default() -> Self {
        Self {
            check_for_updates: true,
            update_uri: None,
        }
    }
}

impl UpdateConfig {
    #[must_use]
    pub fn update_uri(&self) -> Option<&str> {
        self.update_uri
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }
}

/// Resolved CLI configuration, including defaults and environment overrides.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Config {
    pub provider: Option<String>,
    #[serde(alias = "apiKey")]
    pub api_key: Option<String>,
    #[serde(alias = "baseUrl")]
    pub base_url: Option<String>,
    /// Optional extra HTTP headers sent to model API requests.
    #[serde(alias = "httpHeaders")]
    pub http_headers: Option<HashMap<String, String>>,
    #[serde(alias = "defaultTextModel")]
    pub default_text_model: Option<String>,
    #[serde(alias = "authMode")]
    pub auth_mode: Option<String>,
    /// DeepSeek reasoning-effort tier: `"off" | "low" | "medium" | "high" | "max"`.
    /// Defaults to `"max"` at runtime if unset.
    pub reasoning_effort: Option<String>,
    pub tools_file: Option<String>,
    /// Native tool catalog controls. `tools_file` is the legacy external
    /// schema path; this table controls built-in tool loading policy.
    #[serde(default)]
    pub tools: Option<ToolsConfig>,
    pub skills_dir: Option<String>,
    pub mcp_config_path: Option<String>,
    pub mcp_oauth_callback_port: Option<u16>,
    pub mcp_oauth_callback_url: Option<String>,
    pub notes_path: Option<String>,
    /// Legacy single-file memory path (`~/.mimofan/memory.md`). Kept only for
    /// one-time migration into the new directory layout; new code should use
    /// `memory_dir` (see [`memory_dir`]).
    pub memory_path: Option<String>,
    /// Directory holding the categorized memory (`MEMORY.md` index +
    /// `user.md`/`feedback.md`/`project.md`/`reference.md` category files).
    pub memory_dir: Option<String>,
    /// When true, set `tool_choice: "required"` and opt compatible function
    /// schemas into DeepSeek beta strict mode. Schemas with root alternatives
    /// stay non-strict to avoid changing optional/one-of tool semantics.
    pub strict_tool_mode: Option<bool>,
    /// Additional user-owned system-prompt sources concatenated in declared
    /// order (#454). Paths are expanded via `expand_path` so `~` and env vars
    /// work. Project-scope config is not allowed to set this field; the TUI
    /// project overlay ignores `instructions` so a cloned repo cannot choose
    /// arbitrary local files to place into the prompt. Each configured file is
    /// loaded, capped at 100 KiB, and skipped (with a warning) on read errors so
    /// a missing optional file doesn't fail the launch.
    pub instructions: Option<Vec<String>>,
    pub allow_shell: Option<bool>,
    /// Opt-in ghost-text follow-up prompt suggestion after each completed turn.
    /// Default: false — the user must explicitly set this to true to enable.
    pub prompt_suggestion: Option<bool>,
    #[serde(alias = "approvalPolicy")]
    pub approval_policy: Option<String>,
    #[serde(alias = "sandboxMode")]
    pub sandbox_mode: Option<String>,
    #[serde(default, alias = "fallbackProviders")]
    pub fallback_providers: Vec<mimofan_config::ProviderKind>,
    pub yolo: Option<bool>,
    pub verbosity: Option<String>,
    /// External sandbox backend: `"none"` or `"opensandbox"`.
    /// When set, exec_shell routes commands through the backend's HTTP API
    /// instead of spawning a local process.
    #[serde(alias = "sandboxBackend")]
    pub sandbox_backend: Option<String>,
    /// Base URL for the external sandbox backend (default: `"http://localhost:8080"`).
    #[serde(alias = "sandboxUrl")]
    pub sandbox_url: Option<String>,
    /// Optional API key for the external sandbox backend (sent as Bearer token).
    #[serde(alias = "sandboxApiKey")]
    pub sandbox_api_key: Option<String>,
    /// When true and `/usr/bin/bwrap` is present on Linux, route exec_shell
    /// through bubblewrap instead of relying solely on Landlock (#2184).
    /// Defaults to false. Requires the `bubblewrap` package to be installed
    /// separately — we do NOT vendor bwrap.
    #[serde(alias = "preferBwrap")]
    pub prefer_bwrap: Option<bool>,
    #[serde(alias = "managedConfigPath")]
    pub managed_config_path: Option<String>,
    #[serde(alias = "requirementsPath")]
    pub requirements_path: Option<String>,
    #[serde(alias = "maxSubagents")]
    pub max_subagents: Option<usize>,
    pub retry: Option<RetryConfig>,
    pub features: Option<FeaturesToml>,

    /// Deterministic user-level auto-review policy for tool calls. The engine
    /// applies these rules after built-in safety floors, so config cannot
    /// bypass publish/destructive-background holds.
    #[serde(default)]
    pub auto_review: Option<AutoReviewConfig>,

    /// TUI configuration (alternate screen, etc.)
    pub tui: Option<TuiConfig>,

    /// Lifecycle hooks configuration
    #[serde(default)]
    pub hooks: Option<HooksConfig>,

    /// Provider-specific credentials and defaults shared with the `mimofan` facade.
    #[serde(default)]
    pub providers: Option<ProvidersConfig>,

    /// Desktop notification settings (OSC 9 / BEL on long turn completion).
    #[serde(default)]
    pub notifications: Option<NotificationsConfig>,

    /// Per-domain network policy (#135). When absent, network tools fall back
    /// to a permissive default that mirrors pre-v0.7.0 behavior.
    #[serde(default)]
    pub network: Option<NetworkPolicyToml>,

    /// Community skill installer settings (#140). When absent, installer
    /// commands fall back to the bundled defaults
    /// ([`crate::skills::install::DEFAULT_REGISTRY_URL`] +
    /// [`crate::skills::install::DEFAULT_MAX_SIZE_BYTES`]).
    #[serde(default)]
    pub skills: Option<SkillsConfig>,

    /// Workspace side-git snapshots (#137). Defaults to enabled with 7-day
    /// retention when the table is absent.
    #[serde(default)]
    pub snapshots: Option<SnapshotsConfig>,

    /// Web search provider configuration. When absent, defaults to DuckDuckGo.
    /// Set `provider` to another supported backend such as `bing`, `tavily`,
    /// `bocha`, `metaso`, `searxng`, `baidu`, `volcengine`, or `sofya`.
    /// API-backed services require provider-specific credentials; SearXNG
    /// requires a trusted `base_url`.
    #[serde(default)]
    pub search: Option<SearchConfig>,

    /// User-level memory file (#489). Default behaviour is **opt-in**:
    /// loading + injection happens only when `[memory] enabled = true` or
    /// `MIMOFAN_MEMORY=on` is set.
    #[serde(default)]
    pub memory: Option<MemoryConfig>,

    /// Xiaomi MiMo speech/TTS defaults.
    #[serde(default)]
    pub speech: Option<SpeechConfig>,

    /// Tunables for `--model auto` (#1207). When absent, the auto router
    /// keeps its existing balanced behaviour.
    #[serde(default)]
    pub auto: Option<AutoConfig>,

    /// Optional 1-8 hotbar slot bindings (#2064). When absent, hotbar UI and
    /// dispatch layers use the built-in defaults from `mimofan_config`.
    #[serde(default)]
    pub hotbar: Option<Vec<mimofan_config::HotbarBindingToml>>,

    /// Startup update-check behavior. When absent, the TUI keeps the default
    /// fire-and-forget latest-release check.
    #[serde(default)]
    pub update: Option<UpdateConfig>,

    /// Post-edit LSP diagnostics injection (#136). When absent, the engine
    /// applies the defaults documented in [`LspConfigToml`].
    #[serde(default)]
    pub lsp: Option<LspConfigToml>,

    /// Append-only layered context management with Flash seam manager (#159).
    #[serde(default)]
    pub context: ContextConfig,

    /// Agent Fleet trust/security/role/exec config.
    #[serde(default)]
    pub fleet: Option<mimofan_config::FleetConfigToml>,

    /// Sub-agent model overrides.
    #[serde(default)]
    pub subagents: Option<SubagentsConfig>,

    /// Unified concurrency / parallelism / timeout / queue tunables. When
    /// absent, every knob falls back to its documented default.
    #[serde(default)]
    pub limits: Option<LimitsConfig>,

    /// Runtime API server tuning (`mimofan serve --http`). Currently only
    /// hosts the CORS allow-list extension (whalescale#255 / #561). When the
    /// table is absent, the daemon ships with localhost:3000 / localhost:1420
    /// / tauri://localhost as the only allowed dev origins.
    #[serde(default)]
    pub runtime_api: Option<RuntimeApiConfig>,

    /// Workshop / large-tool-output routing (#548). When absent, the global
    /// default threshold of 4 096 tokens applies and routing is active.
    #[serde(default)]
    pub workshop: Option<crate::tools::large_output_router::WorkshopConfig>,

    /// Vision model configuration for the `image_analyze` tool.
    #[serde(default)]
    pub vision_model: Option<VisionModelConfig>,

    /// Sibling `permissions.toml` ask-rules compiled for runtime checks.
    ///
    /// This is deliberately not part of `config.toml`; it is loaded from the
    /// companion permissions file after profile/env/managed config resolution.
    #[serde(skip)]
    pub exec_policy_engine: ExecPolicyEngine,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct AutoReviewConfig {
    #[serde(default, alias = "guidance", alias = "naturalLanguageGuidance")]
    pub natural_language_guidance: Option<String>,
    #[serde(default)]
    pub allow: Vec<AutoReviewRuleConfig>,
    #[serde(default)]
    pub block: Vec<AutoReviewRuleConfig>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct AutoReviewRuleConfig {
    pub id: Option<String>,
    #[serde(default, alias = "toolName", alias = "tool_name")]
    pub tool: Option<String>,
    #[serde(default, alias = "actionKind", alias = "action_kind")]
    pub action_kind: Option<String>,
    #[serde(default, alias = "textContains", alias = "text_contains")]
    pub text_contains: Option<String>,
    pub reason: Option<String>,
}

impl AutoReviewConfig {
    fn to_runtime_policy(&self) -> crate::tui::auto_review::AutoReviewPolicy {
        crate::tui::auto_review::AutoReviewPolicy {
            allow_rules: self
                .allow
                .iter()
                .enumerate()
                .map(|(index, rule)| {
                    rule.to_runtime_rule(index, crate::tui::auto_review::AutoReviewAction::Allow)
                })
                .collect(),
            block_rules: self
                .block
                .iter()
                .enumerate()
                .map(|(index, rule)| {
                    rule.to_runtime_rule(index, crate::tui::auto_review::AutoReviewAction::Block)
                })
                .collect(),
            natural_language_guidance: self
                .natural_language_guidance
                .as_ref()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
        }
    }

    fn validate(&self) -> Result<()> {
        validate_auto_review_rules("allow", &self.allow)?;
        validate_auto_review_rules("block", &self.block)?;
        Ok(())
    }
}

impl AutoReviewRuleConfig {
    fn to_runtime_rule(
        &self,
        index: usize,
        action: crate::tui::auto_review::AutoReviewAction,
    ) -> crate::tui::auto_review::AutoReviewRule {
        let id_prefix = match action {
            crate::tui::auto_review::AutoReviewAction::Allow => "allow",
            crate::tui::auto_review::AutoReviewAction::Block => "block",
            crate::tui::auto_review::AutoReviewAction::AskUser => "ask",
            crate::tui::auto_review::AutoReviewAction::HoldForReview => "hold",
        };
        let id = self
            .id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| format!("config-{id_prefix}-{index}"));
        let reason = self
            .reason
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| format!("configured auto-review {id_prefix} rule"));
        let mut rule = match action {
            crate::tui::auto_review::AutoReviewAction::Allow => {
                crate::tui::auto_review::AutoReviewRule::allow(id, reason)
            }
            crate::tui::auto_review::AutoReviewAction::Block => {
                crate::tui::auto_review::AutoReviewRule::block(id, reason)
            }
            crate::tui::auto_review::AutoReviewAction::AskUser
            | crate::tui::auto_review::AutoReviewAction::HoldForReview => {
                crate::tui::auto_review::AutoReviewRule::block(id, reason)
            }
        };

        if let Some(tool) = self
            .tool
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            rule = rule.tool_name(tool.to_string());
        }
        if let Some(action_kind) = self
            .action_kind
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .and_then(parse_auto_review_action_kind)
        {
            rule = rule.action_kind(action_kind);
        }
        if let Some(text) = self
            .text_contains
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            rule = rule.text_contains(text.to_string());
        }

        rule
    }

    fn has_matcher(&self) -> bool {
        self.tool
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
            || self
                .action_kind
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty())
            || self
                .text_contains
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty())
    }
}

fn validate_auto_review_rules(kind: &str, rules: &[AutoReviewRuleConfig]) -> Result<()> {
    for (index, rule) in rules.iter().enumerate() {
        if !rule.has_matcher() {
            anyhow::bail!(
                "Invalid auto_review.{kind}[{index}]: set at least one of tool, action_kind, or text_contains."
            );
        }
        if let Some(action_kind) = rule.action_kind.as_deref()
            && parse_auto_review_action_kind(action_kind.trim()).is_none()
        {
            anyhow::bail!(
                "Invalid auto_review.{kind}[{index}].action_kind '{action_kind}': expected read, write, shell, network, git, mcp_read, mcp_action, browser, secret, publish, destructive, or unknown."
            );
        }
    }
    Ok(())
}

fn parse_auto_review_action_kind(raw: &str) -> Option<crate::tui::auto_review::ToolActionKind> {
    match raw.trim().to_ascii_lowercase().replace('-', "_").as_str() {
        "read" => Some(crate::tui::auto_review::ToolActionKind::Read),
        "write" => Some(crate::tui::auto_review::ToolActionKind::Write),
        "shell" => Some(crate::tui::auto_review::ToolActionKind::Shell),
        "network" => Some(crate::tui::auto_review::ToolActionKind::Network),
        "git" => Some(crate::tui::auto_review::ToolActionKind::Git),
        "mcp_read" => Some(crate::tui::auto_review::ToolActionKind::McpRead),
        "mcp_action" => Some(crate::tui::auto_review::ToolActionKind::McpAction),
        "browser" => Some(crate::tui::auto_review::ToolActionKind::Browser),
        "secret" => Some(crate::tui::auto_review::ToolActionKind::Secret),
        "publish" => Some(crate::tui::auto_review::ToolActionKind::Publish),
        "destructive" => Some(crate::tui::auto_review::ToolActionKind::Destructive),
        "unknown" => Some(crate::tui::auto_review::ToolActionKind::Unknown),
        _ => None,
    }
}

/// `[runtime_api]` table — knobs for the local HTTP/SSE daemon.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct RuntimeApiConfig {
    /// Additional CORS origins to allow on top of the built-in defaults
    /// (`http://localhost:{3000,1420}`, `http://127.0.0.1:{3000,1420}`,
    /// `tauri://localhost`). Useful when developing a UI against a non-default
    /// dev server port (e.g. Vite's default `:5173`).
    ///
    /// Resolution order (highest priority first): `--cors-origin` CLI flag,
    /// `MIMOFAN_CORS_ORIGINS` env var (comma-separated), this field. Mimofanscale#255 / #561.
    #[serde(default)]
    pub cors_origins: Option<Vec<String>>,
}

/// `[skills]` table — knobs for the community-skill installer.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct SkillsConfig {
    /// Curated registry index. `/skill install <name>` looks up the spec here.
    /// Defaults to [`crate::skills::install::DEFAULT_REGISTRY_URL`].
    #[serde(default)]
    pub registry_url: Option<String>,
    /// Per-skill maximum *uncompressed* size in bytes. Tarballs that exceed
    /// this limit are rejected during validation. Defaults to 5 MiB.
    #[serde(default)]
    pub max_install_size_bytes: Option<u64>,
    /// When true, skill discovery scans only mimofan-owned skill roots
    /// (plus any explicit `skills_dir`) instead of importing compatible
    /// directories from other AI tools such as Claude, OpenCode, or Cursor.
    #[serde(default, alias = "scanMimofanOnly")]
    pub scan_mimofan_only: Option<bool>,
}

impl SkillsConfig {
    /// Resolve the registry URL with the bundled default.
    #[must_use]
    pub fn registry_url(&self) -> String {
        self.registry_url
            .clone()
            .unwrap_or_else(|| crate::skills::install::DEFAULT_REGISTRY_URL.to_string())
    }

    /// Resolve the max install size with the bundled default.
    #[must_use]
    pub fn max_install_size_bytes(&self) -> u64 {
        self.max_install_size_bytes
            .unwrap_or(crate::skills::install::DEFAULT_MAX_SIZE_BYTES)
    }

    /// Resolve whether session-time discovery should ignore cross-tool skill
    /// directories. Defaults to the compatibility-preserving broad scan.
    #[must_use]
    pub fn scan_mimofan_only(&self) -> bool {
        self.scan_mimofan_only.unwrap_or(false)
    }
}

/// `[network]` table — mirrors `mimofan_config::NetworkPolicyToml` so the live
/// TUI runtime can construct a [`crate::network_policy::NetworkPolicy`]
/// without reaching into the workspace config crate. See `config.example.toml`
/// for documentation.
#[derive(Debug, Clone, Deserialize)]
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

impl NetworkPolicyToml {
    /// Build a runtime [`crate::network_policy::NetworkPolicy`] from the
    /// on-disk schema.
    #[must_use]
    pub fn into_runtime(self) -> crate::network_policy::NetworkPolicy {
        crate::network_policy::NetworkPolicy {
            default: crate::network_policy::Decision::parse(&self.default).into(),
            allow: self.allow,
            deny: self.deny,
            proxy: self.proxy,
            audit: self.audit,
        }
    }
}

/// `[lsp]` table — mirrors [`crate::lsp::LspConfig`]. Documented in
/// `config.example.toml`. When omitted, defaults from `LspConfig::default()`
/// apply (enabled, 5 s poll, 20 diagnostics/file, errors only, no overrides).
#[derive(Debug, Clone, Deserialize, Default)]
pub struct LspConfigToml {
    /// Master switch. Defaults to `true`.
    #[serde(default)]
    pub enabled: Option<bool>,
    /// How long to wait for the LSP server to publish diagnostics after a
    /// `didOpen`/`didChange`. Defaults to 5000 ms.
    #[serde(default)]
    pub poll_after_edit_ms: Option<u64>,
    /// Cap on diagnostics surfaced per file. Defaults to 20.
    #[serde(default)]
    pub max_diagnostics_per_file: Option<usize>,
    /// Whether to surface warnings in addition to errors. Defaults to `false`.
    #[serde(default)]
    pub include_warnings: Option<bool>,
    /// Optional override for the `Language -> [cmd, ...args]` table. Keys
    /// are language slugs (`"rust"`, `"go"`, etc.).
    #[serde(default)]
    pub servers: Option<HashMap<String, Vec<String>>>,
}

impl LspConfigToml {
    /// Build a runtime [`crate::lsp::LspConfig`] from the on-disk schema,
    /// falling back to defaults for any unset fields.
    #[must_use]
    pub fn into_runtime(self) -> crate::lsp::LspConfig {
        let defaults = crate::lsp::LspConfig::default();
        crate::lsp::LspConfig {
            enabled: self.enabled.unwrap_or(defaults.enabled),
            poll_after_edit_ms: self
                .poll_after_edit_ms
                .unwrap_or(defaults.poll_after_edit_ms),
            max_diagnostics_per_file: self
                .max_diagnostics_per_file
                .unwrap_or(defaults.max_diagnostics_per_file),
            include_warnings: self.include_warnings.unwrap_or(defaults.include_warnings),
            servers: self.servers.unwrap_or_default(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ProviderConfig {
    #[serde(alias = "apiKey")]
    pub api_key: Option<String>,
    #[serde(alias = "baseUrl")]
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub mode: Option<String>,
    #[serde(alias = "authMode")]
    pub auth_mode: Option<String>,
    #[serde(alias = "insecureSkipTlsVerify")]
    pub insecure_skip_tls_verify: Option<bool>,
    #[serde(alias = "httpHeaders")]
    pub http_headers: Option<HashMap<String, String>>,
    #[serde(alias = "pathSuffix")]
    pub path_suffix: Option<String>,
    #[serde(alias = "reasoningStyle", alias = "reasoningStreamStyle")]
    pub reasoning_stream_style: Option<String>,
    pub auth: Option<mimofan_config::ProviderAuthSourceToml>,
    /// Wire-protocol selector for a custom `[providers.<name>]` entry (#1519).
    ///
    /// Only `"openai-compatible"` is accepted for now; any other value is
    /// rejected at selection time so unsupported wire formats fail loudly rather
    /// than silently routing as OpenAI. Built-in providers leave this unset.
    #[serde(default)]
    pub kind: Option<String>,
    /// Name of the environment variable holding this custom provider's API key
    /// (#1519), e.g. `api_key_env = "EXAMPLE_API_KEY"`. The key value itself is
    /// never stored in config; only the env var name is.
    #[serde(default, alias = "apiKeyEnv")]
    pub api_key_env: Option<String>,
}

impl ProviderConfig {
    /// True when this entry selects the OpenAI-compatible custom wire protocol.
    ///
    /// `kind` is matched case-insensitively against `openai-compatible` (and the
    /// `openai_compatible` underscore spelling). Returns `false` when `kind` is
    /// unset (built-in providers) or names any other value.
    #[must_use]
    pub fn is_openai_compatible_custom(&self) -> bool {
        self.kind.as_deref().is_some_and(|kind| {
            let normalized = kind.trim().to_ascii_lowercase().replace('_', "-");
            normalized == "openai-compatible"
        })
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ProvidersConfig {
    /// OpenAI-compatible `/v1/chat/completions` endpoint config.
    #[serde(default)]
    pub openai_compatible: ProviderConfig,
    /// Anthropic Messages API compatible endpoint (`/v1/messages`).
    #[serde(default)]
    pub anthropic_compatible: ProviderConfig,
    /// Google Gemini compatible endpoint config.
    #[serde(default)]
    pub gemini_compatible: ProviderConfig,
    /// Arbitrary user-named custom providers (#1519).
    ///
    /// Captures every `[providers.<name>]` table. Each entry is an
    /// OpenAI-compatible custom endpoint selected via `provider = "<name>"`;
    /// routing reads its `base_url` / `model` / `api_key_env` through
    /// [`ApiProvider::OpenAiCompatible`].
    #[serde(flatten, default)]
    pub custom: HashMap<String, ProviderConfig>,
}

impl ProvidersConfig {
    /// Look up a user-defined custom provider table by its `[providers.<name>]`
    /// key (#1519). Returns `None` when no entry with that exact name exists.
    #[must_use]
    pub fn custom_provider_config(&self, name: &str) -> Option<&ProviderConfig> {
        self.custom.get(name)
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
struct ConfigFile {
    #[serde(flatten)]
    base: Config,
    profiles: Option<HashMap<String, Config>>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct RequirementsFile {
    #[serde(default)]
    allowed_approval_policies: Vec<String>,
    #[serde(default)]
    allowed_sandbox_modes: Vec<String>,
}

// === Config Loading ===

impl Config {
    #[must_use]
    pub fn search_provider_resolution(&self) -> SearchProviderResolution {
        if let Ok(raw) = std::env::var("MIMOFAN_SEARCH_PROVIDER")
            && let Some(provider) = SearchProvider::parse(&raw)
        {
            return SearchProviderResolution {
                provider,
                source: SearchProviderSource::EnvOverride,
            };
        }

        if let Some(provider) = self.search.as_ref().and_then(|search| search.provider) {
            return SearchProviderResolution {
                provider,
                source: SearchProviderSource::Config,
            };
        }

        SearchProviderResolution {
            provider: SearchProvider::default(),
            source: SearchProviderSource::Default,
        }
    }

    #[must_use]
    pub fn search_provider(&self) -> SearchProvider {
        self.search_provider_resolution().provider
    }

    /// Return `true` if the `[auto] cost_saving = true` opt-in is set
    /// (#1207). When true, the auto-mode router biases toward
    /// `deepseek-v4-flash` for ambiguous requests instead of escalating to
    /// `deepseek-v4-pro`. Default: `false` (balanced behaviour).
    #[must_use]
    pub fn auto_cost_saving(&self) -> bool {
        self.auto
            .as_ref()
            .and_then(|a| a.cost_saving)
            .unwrap_or(false)
    }

    #[must_use]
    pub fn tools_always_load(&self) -> std::collections::HashSet<String> {
        self.tools
            .as_ref()
            .map(|tools| {
                tools
                    .always_load
                    .iter()
                    .map(|name| name.trim())
                    .filter(|name| !name.is_empty())
                    .map(ToOwned::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    }

    #[must_use]
    pub fn auto_review_policy(&self) -> crate::tui::auto_review::AutoReviewPolicy {
        self.auto_review
            .as_ref()
            .map(AutoReviewConfig::to_runtime_policy)
            .unwrap_or_default()
    }

    /// Load configuration from disk and merge with environment overrides.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use crate::config::Config;
    /// let config = Config::load(None, None)?;
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn load(path: Option<PathBuf>, profile: Option<&str>) -> Result<Self> {
        let path = resolve_load_config_path(path);
        let mut config = if let Some(path) = path.as_ref() {
            if path.exists() {
                let contents = fs::read_to_string(path)
                    .with_context(|| format!("Failed to read config file: {}", path.display()))?;
                let parsed: ConfigFile = toml::from_str(&contents)
                    .with_context(|| format!("Failed to parse config file: {}", path.display()))?;
                if let Some(msg) = warn_on_misplaced_top_level_keys(&contents) {
                    tracing::warn!("{msg}");
                }
                apply_profile(parsed, profile)?
            } else {
                Config::default()
            }
        } else {
            Config::default()
        };

        apply_env_overrides(&mut config);
        apply_managed_overrides(&mut config)?;
        apply_requirements(&mut config)?;
        config.exec_policy_engine = load_sibling_exec_policy_engine(path.as_deref())?;
        config.validate()?;
        config.warn_on_misplaced_root_base_url();
        Ok(config)
    }

    /// Surface a one-line warning when the user has set the legacy root
    /// `base_url` field. The active base URL is always resolved from the
    /// per-provider `[providers.<mode>]` table (or the matching `*_BASE_URL`
    /// env var / runtime override); a top-level `base_url` is ignored (#1308).
    fn warn_on_misplaced_root_base_url(&self) {
        let Some(root_base) = self.base_url.as_deref().map(str::trim) else {
            return;
        };
        if root_base.is_empty() {
            return;
        }
        let provider = self.api_provider();
        // Only warn if the per-provider table doesn't have an explicit
        // `base_url`, because if it does, the per-provider one wins and the
        // root field is just dead config — no behavior surprise.
        let has_provider_base = self
            .provider_config_for(provider)
            .and_then(|p| p.base_url.as_deref().map(str::trim))
            .is_some_and(|s| !s.is_empty());
        if has_provider_base {
            return;
        }
        let Ok(table) = provider_config_table_name(provider) else {
            return;
        };
        tracing::warn!(
            "Top-level `base_url = \"{root_base}\"` is ignored for the {provider:?} provider. \
             Move it under `[{table}]` (e.g. `[{table}]\\nbase_url = \"...\"`) \
             or set the corresponding `*_BASE_URL` env var. (#1308)"
        );
    }

    /// Validate that critical config fields are present.
    pub fn validate(&self) -> Result<()> {
        if let Some(provider) = self.provider.as_deref()
            && ApiProvider::parse(provider).is_none()
        {
            anyhow::bail!(
                "Invalid provider '{provider}': expected {}.",
                ApiProvider::names_hint()
            );
        }
        if let Some(ref key) = self.api_key
            && key.trim().is_empty()
        {
            anyhow::bail!("api_key cannot be empty string");
        }
        if let Some(features) = &self.features {
            for key in features.entries.keys() {
                if !is_known_feature_key(key) {
                    anyhow::bail!("Unknown feature flag: {key}");
                }
            }
        }
        if let Some(model) = self.default_text_model.as_deref()
            && !model.trim().eq_ignore_ascii_case("auto")
            && !provider_passes_model_through(self.api_provider())
            && normalize_model_name(model).is_none()
        {
            anyhow::bail!(
                "Invalid default_text_model '{model}': expected auto or a model ID supported by the {provider} provider.",
                provider = self.api_provider().as_str()
            );
        }
        if let Some(policy) = self.approval_policy.as_deref() {
            let normalized = policy.trim().to_ascii_lowercase();
            if !matches!(
                normalized.as_str(),
                "on-request" | "untrusted" | "never" | "auto" | "suggest"
            ) {
                anyhow::bail!(
                    "Invalid approval_policy '{policy}': expected on-request, untrusted, never, auto, or suggest."
                );
            }
        }
        if let Some(v) = self.verbosity.as_deref() {
            let normalized = v.trim().to_ascii_lowercase();
            if !matches!(normalized.as_str(), "normal" | "concise") {
                anyhow::bail!("Invalid verbosity '{v}': expected normal or concise.");
            }
        }
        if let Some(mode) = self.sandbox_mode.as_deref() {
            let normalized = mode.trim().to_ascii_lowercase();
            if !matches!(
                normalized.as_str(),
                "read-only" | "workspace-write" | "danger-full-access" | "external-sandbox"
            ) {
                anyhow::bail!(
                    "Invalid sandbox_mode '{mode}': expected read-only, workspace-write, danger-full-access, or external-sandbox."
                );
            }
        }
        if let Some(tui) = &self.tui
            && let Some(mode) = tui.alternate_screen.as_deref()
        {
            let mode = mode.to_ascii_lowercase();
            if !matches!(mode.as_str(), "auto" | "always" | "never") {
                anyhow::bail!(
                    "Invalid tui.alternate_screen '{mode}': expected auto, always, or never."
                );
            }
        }
        if let Some(auto_review) = &self.auto_review {
            auto_review.validate()?;
        }
        Ok(())
    }

    #[must_use]
    pub fn api_provider(&self) -> ApiProvider {
        if let Some(provider) = self.provider.as_deref().and_then(ApiProvider::parse) {
            return provider;
        }
        if let Some(name) = self.provider.as_deref()
            && self
                .providers
                .as_ref()
                .and_then(|providers| providers.custom_provider_config(name))
                .is_some()
        {
            return ApiProvider::OpenAiCompatible;
        }
        ApiProvider::OpenAiCompatible
    }

    pub(crate) fn provider_config_for(&self, provider: ApiProvider) -> Option<&ProviderConfig> {
        let providers = self.providers.as_ref()?;
        if provider == ApiProvider::OpenAiCompatible
            && let Some(name) = self.provider.as_deref()
            && providers.custom.contains_key(name)
        {
            return providers.custom_provider_config(name);
        }
        Some(match provider {
            ApiProvider::OpenAiCompatible => &providers.openai_compatible,
            ApiProvider::AnthropicCompatible => &providers.anthropic_compatible,
            ApiProvider::GeminiCompatible => &providers.gemini_compatible,
        })
    }

    pub(crate) fn subagent_provider_config(
        &self,
        provider: ApiProvider,
    ) -> Option<&SubagentProviderConfig> {
        let providers = self.subagents.as_ref()?.providers.as_ref()?;
        providers.iter().find_map(|(key, config)| {
            subagent_provider_key_matches(key, provider).then_some(config)
        })
    }

    pub(crate) fn provider_config_for_mut(&mut self, provider: ApiProvider) -> &mut ProviderConfig {
        let providers = self.providers.get_or_insert_with(ProvidersConfig::default);
        match provider {
            ApiProvider::OpenAiCompatible => {
                if let Some(name) = self.provider.clone()
                    && providers.custom.contains_key(&name)
                {
                    return providers.custom.entry(name).or_default();
                }
                &mut providers.openai_compatible
            }
            ApiProvider::AnthropicCompatible => &mut providers.anthropic_compatible,
            ApiProvider::GeminiCompatible => &mut providers.gemini_compatible,
        }
    }

    pub(crate) fn provider_config(&self) -> Option<&ProviderConfig> {
        self.provider_config_for(self.api_provider())
    }

    fn provider_config_string_with_runtime_fallback<F>(
        &self,
        provider: ApiProvider,
        get: F,
    ) -> Option<String>
    where
        F: Fn(&ProviderConfig) -> Option<String>,
    {
        self.provider_config_for(provider).and_then(&get)
    }

    #[must_use]
    pub fn insecure_skip_tls_verify(&self) -> bool {
        self.provider_config()
            .and_then(|provider| provider.insecure_skip_tls_verify)
            .unwrap_or(false)
    }

    #[must_use]
    pub fn http_headers(&self) -> HashMap<String, String> {
        let mut headers = self.http_headers.clone().unwrap_or_default();
        if let Some(provider_headers) = self
            .provider_config()
            .and_then(|provider| provider.http_headers.as_ref())
        {
            headers.extend(provider_headers.clone());
        }
        headers.retain(|name, value| !name.trim().is_empty() && !value.trim().is_empty());
        headers
    }

    #[must_use]
    pub fn default_model(&self) -> String {
        let provider = self.api_provider();
        if let Some(model) =
            self.provider_config_string_with_runtime_fallback(provider, |entry| entry.model.clone())
        {
            let model = model.trim();
            if provider_passes_model_through(provider)
                || self.active_provider_preserves_custom_base_url_model()
            {
                return model.to_string();
            }
            if let Some(normalized) = normalize_model_for_provider(provider, model) {
                return normalized;
            }
            if !model.is_empty() {
                return model.to_string();
            }
        }
        if let Some(model) = self.default_text_model.as_deref()
            && model.trim().eq_ignore_ascii_case("auto")
        {
            return "auto".to_string();
        }
        if let Some(model) = self.default_text_model.as_deref()
            && (provider_passes_model_through(provider)
                || self.active_provider_preserves_custom_base_url_model())
        {
            return model.trim().to_string();
        }
        if let Some(model) = self.default_text_model.as_deref()
            && let Some(normalized) = normalize_model_name_for_provider(provider, model)
        {
            return normalized;
        }

        provider
            .kind()
            .map(|kind| kind.provider().default_model().to_string())
            .unwrap_or_else(|| DEFAULT_TEXT_MODEL.to_string())
    }

    /// Return the configured API base URL (normalized).
    #[must_use]
    pub fn api_base_url(&self) -> String {
        let provider = self.api_provider();
        let provider_base = self
            .provider_config_string_with_runtime_fallback(provider, |entry| entry.base_url.clone());
        let configured_base_url = provider_base;
        let base = configured_base_url
            .or_else(env_base_url_override)
            .unwrap_or_else(|| default_base_url_for_provider(provider).to_string());
        normalize_base_url(&base)
    }

    fn active_provider_preserves_custom_base_url_model(&self) -> bool {
        let provider = self.api_provider();
        provider_preserves_custom_base_url_model(provider, &self.api_base_url())
    }

    pub(crate) fn model_ids_pass_through(&self) -> bool {
        let provider = self.api_provider();
        provider_passes_model_through(provider)
            || self.active_provider_preserves_custom_base_url_model()
    }

    /// Read the API key.
    ///
    /// Precedence: **explicit in-memory override → provider/root config
    /// → environment**.
    ///
    /// The in-memory `self.api_key` override is only honored when the user
    /// explicitly set the field (not the legacy `API_KEYRING_SENTINEL`
    /// placeholder, not empty whitespace).
    pub fn api_key(&self) -> Result<String> {
        let provider = self.api_provider();

        // 1. Config file (provider-scoped slot). This intentionally wins
        // over ambient env so `mimofan auth set` fixes stale shell exports.
        if let Some(configured) = self
            .provider_config_string_with_runtime_fallback(provider, |entry| entry.api_key.clone())
            && !configured.trim().is_empty()
        {
            return Ok(configured);
        }

        // 1b. Custom (named OpenAI-compatible) providers (#1519) name their
        // auth env var per-entry via `[providers.<name>] api_key_env = "..."`.
        // Resolve it before the generic env step, since the custom identity
        // declares no built-in env var. The env var NAME is read from config;
        // the secret value is read from the process environment and never
        // persisted.
        if provider == ApiProvider::OpenAiCompatible
            && let Some(name) = self.provider.clone()
            && self
                .providers
                .as_ref()
                .is_some_and(|providers| providers.custom.contains_key(&name))
            && let Some(env_name) = self
                .provider_config_for(provider)
                .and_then(|entry| entry.api_key_env.as_deref())
                .map(str::trim)
                .filter(|name| !name.is_empty())
            && let Ok(value) = std::env::var(env_name)
            && !value.trim().is_empty()
        {
            return Ok(value);
        }

        // 2. Environment variables. Do not query platform credential stores
        // here; routine startup and doctor checks must stay prompt-free.
        if let Some(value) = provider_env_api_key(provider) {
            return Ok(value);
        }

        if base_url_uses_local_host(&self.api_base_url()) {
            return Ok(String::new());
        }

        match provider {
            ApiProvider::OpenAiCompatible => {
                let provider_name = self.provider.as_deref().unwrap_or("<name>");
                if self
                    .providers
                    .as_ref()
                    .is_some_and(|providers| providers.custom.contains_key(provider_name))
                {
                    match self
                        .provider_config_for(provider)
                        .and_then(|entry| entry.api_key_env.as_deref())
                        .map(str::trim)
                        .filter(|name| !name.is_empty())
                    {
                        Some(env_name) => anyhow::bail!(
                            "Custom provider '{provider_name}' API key not found.\n\
                             Set the environment variable {env_name} to your key, \
                             or add api_key to [providers.{provider_name}]."
                        ),
                        None => anyhow::bail!(
                            "Custom provider '{provider_name}' has no auth configured.\n\
                             Add api_key_env = \"YOUR_ENV_VAR\" (or api_key) to \
                             [providers.{provider_name}] in ~/.mimofan/config.toml."
                        ),
                    }
                }
                anyhow::bail!("{}", missing_provider_api_key_message(provider)?)
            }
            _ => anyhow::bail!("{}", missing_provider_api_key_message(provider)?),
        }
    }

    /// Resolve the skills directory path.
    #[must_use]
    pub fn skills_dir(&self) -> PathBuf {
        self.skills_dir
            .as_deref()
            .map(expand_path)
            .or_else(default_skills_dir)
            .unwrap_or_else(|| PathBuf::from("./skills"))
    }

    /// Resolve the MCP config path.
    #[must_use]
    pub fn mcp_config_path(&self) -> PathBuf {
        self.mcp_config_path
            .as_deref()
            .map(expand_path)
            .or_else(default_mcp_config_path)
            .unwrap_or_else(|| PathBuf::from("./mcp.json"))
    }

    /// Resolve the notes file path.
    #[must_use]
    pub fn notes_path(&self) -> PathBuf {
        self.notes_path
            .as_deref()
            .map(expand_path)
            .or_else(default_notes_path)
            .unwrap_or_else(|| PathBuf::from("./notes.txt"))
    }

    /// Resolve the legacy single-file memory path (for migration only).
    #[must_use]
    pub fn memory_path(&self) -> PathBuf {
        self.memory_path
            .as_deref()
            .map(expand_path)
            .or_else(default_memory_path)
            .unwrap_or_else(|| PathBuf::from("./memory.md"))
    }

    /// Resolve the categorized memory directory. Falls back to the default
    /// `~/.mimofan/memory` when unset.
    #[must_use]
    pub fn memory_dir(&self) -> PathBuf {
        self.memory_dir
            .as_deref()
            .map(expand_path)
            .or_else(default_memory_dir)
            .unwrap_or_else(|| PathBuf::from("./memory"))
    }

    /// Resolve the default speech/TTS output directory, if configured.
    #[must_use]
    pub fn speech_output_dir(&self) -> Option<PathBuf> {
        std::env::var("MIMOFAN_SPEECH_OUTPUT_DIR")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .map(|value| expand_path(&value))
            .or_else(|| {
                self.speech
                    .as_ref()
                    .and_then(|speech| speech.output_dir.as_deref())
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(expand_path)
            })
    }

    /// Resolve the configured `instructions = [...]` array (#454)
    /// to absolute paths, in declared order. Empty when unset or
    /// when every entry is empty after trimming. Each entry runs
    /// through `expand_path` so `~` and env vars are honoured.
    #[must_use]
    pub fn instructions_paths(&self) -> Vec<PathBuf> {
        self.instructions
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .map(String::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(expand_path)
            .collect()
    }

    /// Whether the user-memory feature is enabled. The default is **off**
    /// to preserve zero-overhead behavior for users who haven't opted in.
    /// Flips to `true` when `[memory] enabled = true` in `config.toml` or
    /// `MIMOFAN_MEMORY=on` is set in the environment.
    #[must_use]
    pub fn memory_enabled(&self) -> bool {
        self.memory
            .as_ref()
            .and_then(|m| m.enabled)
            .unwrap_or(false)
    }

    /// Return the configured vision model config, inheriting api_key from main config.
    #[must_use]
    pub fn vision_model_config(&self) -> Option<VisionModelConfig> {
        let mut config = self.vision_model.clone()?;
        if config.api_key.is_none() {
            config.api_key = self.api_key.clone();
        }
        Some(config)
    }

    #[must_use]
    pub fn project_context_pack_enabled(&self) -> bool {
        self.context.project_pack.unwrap_or(true)
    }

    /// Return whether shell execution is allowed. Defaults to `false`: shell
    /// access must be opted into explicitly (GHSA-72w5-pf8h-xfp4).
    #[must_use]
    pub fn allow_shell(&self) -> bool {
        self.allow_shell.unwrap_or(false)
    }

    /// Whether ghost-text prompt suggestion is enabled (opt-in, default off).
    pub fn prompt_suggestion_enabled(&self) -> bool {
        self.prompt_suggestion.unwrap_or(false)
    }

    /// Return the maximum number of concurrent sub-agents.
    /// Reads `[limits] max_subagents` first, then the legacy top-level
    /// `max_subagents`, then falls back to `DEFAULT_MAX_SUBAGENTS`. Both are
    /// clamped to `[1, MAX_SUBAGENTS]`.
    #[must_use]
    pub fn max_subagents(&self) -> usize {
        self.limits
            .as_ref()
            .and_then(|cfg| cfg.max_subagents)
            .or(self.max_subagents)
            .unwrap_or(DEFAULT_MAX_SUBAGENTS)
            .clamp(1, MAX_SUBAGENTS)
    }

    /// Return the provider-specific maximum number of concurrent sub-agents.
    /// `[subagents.providers.<provider>] max_concurrent` inherits from the
    /// global `[subagents]` value when unset.
    #[must_use]
    pub fn max_subagents_for_provider(&self, provider: ApiProvider) -> usize {
        self.subagent_provider_config(provider)
            .and_then(|cfg| cfg.max_concurrent)
            .map(|max| max.clamp(1, MAX_SUBAGENTS))
            .unwrap_or_else(|| self.max_subagents())
    }

    /// Whether the model-facing `agent` tool is available after applying the
    /// feature flag, explicit `[subagents] enabled` switch, and legacy
    /// zero-valued opt-outs.
    #[must_use]
    pub fn subagents_enabled(&self) -> bool {
        self.subagents_disabled_reason().is_none()
    }

    /// Whether the model-facing `agent` tool is available for this provider
    /// after applying global and provider-specific sub-agent controls.
    #[must_use]
    pub fn subagents_enabled_for_provider(&self, provider: ApiProvider) -> bool {
        if !self.subagents_enabled() {
            return false;
        }
        let Some(provider_cfg) = self.subagent_provider_config(provider) else {
            return true;
        };
        provider_cfg.enabled != Some(false)
            && provider_cfg.max_concurrent != Some(0)
            && provider_cfg.max_depth != Some(0)
    }

    /// Machine-readable reason sub-agents are disabled, in precedence order.
    #[must_use]
    pub fn subagents_disabled_reason(&self) -> Option<&'static str> {
        if !self.features().enabled(Feature::Subagents) {
            return Some("features.subagents=false");
        }
        if let Some(subagents_cfg) = self.subagents.as_ref() {
            if subagents_cfg.enabled == Some(false) {
                return Some("subagents.enabled=false");
            }
            if subagents_cfg.max_depth == Some(0) {
                return Some("subagents.max_depth=0");
            }
        }
        if self.limits.as_ref().and_then(|cfg| cfg.max_subagents) == Some(0) {
            return Some("limits.max_subagents=0");
        }
        None
    }

    /// How many levels of nested sub-agents the interactive `agent` tool may
    /// spawn. Reads `[subagents] max_depth`; when unset it defaults to
    /// [`mimofan_config::DEFAULT_SPAWN_DEPTH`]. `0` is a valid value that
    /// blocks the `agent` tool at this runtime depth. Any value is clamped to
    /// [`mimofan_config::MAX_SPAWN_DEPTH_CEILING`] so the operator's choice
    /// can never exceed the hard recursion ceiling.
    #[must_use]
    pub fn subagent_max_spawn_depth(&self) -> u32 {
        self.subagents
            .as_ref()
            .and_then(|cfg| cfg.max_depth)
            .unwrap_or(mimofan_config::DEFAULT_SPAWN_DEPTH)
            .min(mimofan_config::MAX_SPAWN_DEPTH_CEILING)
    }

    /// Return the provider-specific maximum sub-agent recursion depth.
    #[must_use]
    pub fn subagent_max_spawn_depth_for_provider(&self, provider: ApiProvider) -> u32 {
        self.subagent_provider_config(provider)
            .and_then(|cfg| cfg.max_depth)
            .unwrap_or_else(|| self.subagent_max_spawn_depth())
            .min(mimofan_config::MAX_SPAWN_DEPTH_CEILING)
    }

    /// Number of direct (depth-1) sub-agents that may execute concurrently
    /// before further launches queue for a launch slot (#3095). Reads
    /// `[subagents] launch_concurrency` (or the deprecated
    /// `interactive_max_launch` alias); when unset it defaults to the full
    /// resolved `max_subagents()` (no artificial throttle), and any explicit
    /// value is clamped to `[1, max_subagents]`.
    #[must_use]
    pub fn launch_concurrency(&self) -> usize {
        let max = self.max_subagents();
        self.limits
            .as_ref()
            .and_then(|cfg| cfg.launch_concurrency)
            .unwrap_or(max)
            .clamp(1, max)
    }

    /// Return the provider-specific direct launch throttle. Children above
    /// this limit queue for a launch slot instead of starting immediately.
    #[must_use]
    pub fn launch_concurrency_for_provider(&self, provider: ApiProvider) -> usize {
        let max = self.max_subagents_for_provider(provider);
        self.subagent_provider_config(provider)
            .and_then(|cfg| cfg.launch_concurrency)
            .or_else(|| self.limits.as_ref().and_then(|cfg| cfg.launch_concurrency))
            .unwrap_or(max)
            .clamp(1, max)
    }

    /// Maximum queued + running sub-agents admitted for the session.
    ///
    /// Defaults to [`MAX_SUBAGENT_ADMISSION`] so distinct `agent` calls can
    /// queue and drain through `launch_concurrency` instead of being rejected
    /// at the instantaneous concurrency cap. Explicit values are clamped to
    /// `[max_subagents, MAX_SUBAGENT_ADMISSION]`.
    #[must_use]
    pub fn max_admitted_subagents(&self) -> usize {
        let max_concurrent = self.max_subagents();
        self.limits
            .as_ref()
            .and_then(|cfg| cfg.max_admitted_subagents)
            .unwrap_or(MAX_SUBAGENT_ADMISSION)
            .clamp(max_concurrent, MAX_SUBAGENT_ADMISSION)
    }

    /// Return the provider-specific queued + running admission cap.
    #[must_use]
    pub fn max_admitted_subagents_for_provider(&self, provider: ApiProvider) -> usize {
        let max_concurrent = self.max_subagents_for_provider(provider);
        self.subagent_provider_config(provider)
            .and_then(|cfg| cfg.max_admitted)
            .or_else(|| {
                self.limits
                    .as_ref()
                    .and_then(|cfg| cfg.max_admitted_subagents)
            })
            .unwrap_or(MAX_SUBAGENT_ADMISSION)
            .clamp(max_concurrent, MAX_SUBAGENT_ADMISSION)
    }

    /// Optional aggregate token budget for each root `agent` run.
    ///
    /// Reads `[subagents] token_budget`. `None` and `0` both mean unlimited,
    /// preserving legacy behavior until a budget is explicitly configured.
    #[must_use]
    pub fn subagent_token_budget(&self) -> Option<u64> {
        self.subagents
            .as_ref()
            .and_then(|cfg| cfg.token_budget)
            .filter(|budget| *budget > 0)
    }

    /// Return the provider-specific aggregate token budget for each root
    /// `agent` run.
    #[must_use]
    pub fn subagent_token_budget_for_provider(&self, provider: ApiProvider) -> Option<u64> {
        self.subagent_provider_config(provider)
            .and_then(|cfg| cfg.token_budget)
            .or_else(|| self.subagents.as_ref().and_then(|cfg| cfg.token_budget))
            .filter(|budget| *budget > 0)
    }

    /// Resolved per-step DeepSeek API timeout for sub-agents, in seconds.
    ///
    /// Reads `[subagents] api_timeout_secs` and clamps to
    /// `[MIN_SUBAGENT_API_TIMEOUT_SECS, MAX_SUBAGENT_API_TIMEOUT_SECS]`
    /// (1..=1800). `None` or `0` resolve to the legacy
    /// `DEFAULT_SUBAGENT_API_TIMEOUT_SECS` (120) so existing configs keep
    /// their old behavior; explicit `1` is honored, useful only in fast
    /// fail-fast tests, not production (#1806, #1808).
    #[must_use]
    pub fn subagent_api_timeout_secs(&self) -> u64 {
        resolve_subagent_api_timeout_secs(self.limits.as_ref().and_then(|cfg| cfg.api_timeout_secs))
    }

    /// Return the provider-specific per-step API timeout for sub-agents.
    #[must_use]
    pub fn subagent_api_timeout_secs_for_provider(&self, provider: ApiProvider) -> u64 {
        resolve_subagent_api_timeout_secs(
            self.subagent_provider_config(provider)
                .and_then(|cfg| cfg.api_timeout_secs)
                .or_else(|| self.limits.as_ref().and_then(|cfg| cfg.api_timeout_secs)),
        )
    }

    /// Resolved no-progress heartbeat timeout for running sub-agents.
    ///
    /// Reads `[limits] heartbeat_timeout_secs` and clamps to
    /// `[MIN_SUBAGENT_HEARTBEAT_TIMEOUT_SECS, MAX_SUBAGENT_HEARTBEAT_TIMEOUT_SECS]`.
    /// `None` or `0` resolve to the default 300 seconds. The final value is
    /// also kept at least 30 seconds above `subagent_api_timeout_secs()` so a
    /// configured long model request is not pre-empted by heartbeat cleanup.
    #[must_use]
    pub fn subagent_heartbeat_timeout_secs(&self) -> u64 {
        resolve_subagent_heartbeat_timeout_secs(
            self.limits
                .as_ref()
                .and_then(|cfg| cfg.heartbeat_timeout_secs),
            self.subagent_api_timeout_secs(),
        )
    }

    /// Return the provider-specific no-progress heartbeat timeout.
    #[must_use]
    pub fn subagent_heartbeat_timeout_secs_for_provider(&self, provider: ApiProvider) -> u64 {
        let api_timeout = self.subagent_api_timeout_secs_for_provider(provider);
        resolve_subagent_heartbeat_timeout_secs(
            self.subagent_provider_config(provider)
                .and_then(|cfg| cfg.heartbeat_timeout_secs)
                .or_else(|| {
                    self.limits
                        .as_ref()
                        .and_then(|cfg| cfg.heartbeat_timeout_secs)
                }),
            api_timeout,
        )
    }

    /// Resolved per-SSE-chunk idle timeout in seconds.
    ///
    /// Reads `[tui].stream_chunk_timeout_secs`, falling back to the legacy
    /// `MIMOFAN_STREAM_IDLE_TIMEOUT_SECS` env var when the config key is
    /// omitted. `None` or `0` resolve to the default 300 seconds; explicit
    /// values are clamped to `1..=3600`.
    #[must_use]
    pub fn stream_chunk_timeout_secs(&self) -> u64 {
        let raw = self
            .tui
            .as_ref()
            .and_then(|cfg| cfg.stream_chunk_timeout_secs)
            .or_else(|| {
                std::env::var(STREAM_CHUNK_TIMEOUT_ENV)
                    .ok()
                    .and_then(|value| value.parse::<u64>().ok())
            })
            .unwrap_or(DEFAULT_STREAM_CHUNK_TIMEOUT_SECS);
        if raw == 0 {
            return DEFAULT_STREAM_CHUNK_TIMEOUT_SECS;
        }
        raw.clamp(MIN_STREAM_CHUNK_TIMEOUT_SECS, MAX_STREAM_CHUNK_TIMEOUT_SECS)
    }

    /// Raw sub-agent model override map. Values are validated at spawn time
    /// so an invalid role/type model fails before any partial agent spawn.
    #[must_use]
    pub fn subagent_model_overrides(&self) -> HashMap<String, String> {
        let mut overrides = HashMap::new();
        let Some(cfg) = self.subagents.as_ref() else {
            return overrides;
        };

        let mut insert = |key: &str, value: &Option<String>| {
            if let Some(model) = value.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
                overrides.insert(key.to_string(), model.to_string());
            }
        };
        insert("default", &cfg.default_model);
        insert("worker", &cfg.worker_model);
        insert("general", &cfg.worker_model);
        insert("explorer", &cfg.explorer_model);
        insert("explore", &cfg.explorer_model);
        insert("awaiter", &cfg.awaiter_model);
        insert("plan", &cfg.awaiter_model);
        insert("review", &cfg.review_model);
        insert("custom", &cfg.custom_model);

        if let Some(models) = cfg.models.as_ref() {
            for (key, model) in models {
                let key = key.trim();
                let model = model.trim();
                if !key.is_empty() && !model.is_empty() {
                    overrides.insert(key.to_ascii_lowercase(), model.to_string());
                }
            }
        }

        overrides
    }

    /// Return the configured DeepSeek reasoning-effort tier, if any.
    #[must_use]
    pub fn reasoning_effort(&self) -> Option<&str> {
        self.reasoning_effort.as_deref()
    }

    /// Get hooks configuration, returning default if not configured.
    pub fn hooks_config(&self) -> HooksConfig {
        self.hooks.clone().unwrap_or_default()
    }

    /// Resolve the notifications configuration with defaults applied.
    #[must_use]
    pub fn notifications_config(&self) -> NotificationsConfig {
        self.notifications.clone().unwrap_or_default()
    }

    /// Resolve workspace side-git snapshot settings with defaults applied.
    #[must_use]
    pub fn snapshots_config(&self) -> SnapshotsConfig {
        self.snapshots.clone().unwrap_or_default()
    }

    /// Resolve community skill settings with defaults applied.
    #[must_use]
    pub fn skills_config(&self) -> SkillsConfig {
        self.skills.clone().unwrap_or_default()
    }

    /// Resolve startup update-check settings with defaults applied.
    #[must_use]
    pub fn update_config(&self) -> UpdateConfig {
        self.update.clone().unwrap_or_default()
    }

    /// Resolve durable hotbar bindings for render/dispatch layers.
    #[must_use]
    pub fn resolve_hotbar_bindings(
        &self,
        known_action_ids: &[&str],
    ) -> mimofan_config::HotbarConfigResolution {
        mimofan_config::resolve_hotbar_bindings(self.hotbar.as_deref(), known_action_ids)
    }

    /// Resolve enabled features from defaults and config entries.
    #[must_use]
    pub fn features(&self) -> Features {
        let mut features = Features::with_defaults();
        if let Some(table) = &self.features {
            features.apply_map(&table.entries);
        }
        features
    }

    /// Override a feature flag in memory (used by CLI overrides).
    pub fn set_feature(&mut self, key: &str, enabled: bool) -> Result<()> {
        if !is_known_feature_key(key) {
            anyhow::bail!("Unknown feature flag: {key}");
        }
        let table = self.features.get_or_insert_with(FeaturesToml::default);
        table.entries.insert(key.to_string(), enabled);
        Ok(())
    }

    /// Resolve the effective retry policy with defaults applied.
    #[must_use]
    pub fn retry_policy(&self) -> RetryPolicy {
        let defaults = RetryPolicy {
            enabled: true,
            max_retries: 3,
            initial_delay: 1.0,
            max_delay: 60.0,
            exponential_base: 2.0,
        };

        let Some(cfg) = &self.retry else {
            return defaults;
        };

        RetryPolicy {
            enabled: cfg.enabled.unwrap_or(defaults.enabled),
            max_retries: cfg.max_retries.unwrap_or(defaults.max_retries),
            initial_delay: cfg.initial_delay.unwrap_or(defaults.initial_delay),
            max_delay: cfg.max_delay.unwrap_or(defaults.max_delay),
            exponential_base: cfg.exponential_base.unwrap_or(defaults.exponential_base),
        }
    }
}

// === Defaults ===

// Pure filesystem path helpers live in the `paths` leaf module. The two
// `pub(crate)` entry points are re-exported so external `crate::config::`
// callers resolve unchanged; the remaining helpers are imported privately for
// the workspace-trust/config-load logic that stays in this file (#3311).
mod paths;
use paths::{
    default_config_path, default_managed_config_path, default_mcp_config_path, default_memory_dir,
    default_memory_path, default_notes_path, default_requirements_path, default_skills_dir,
    env_config_path, expand_pathbuf, home_config_path,
};
pub(crate) use paths::{effective_home_dir, expand_path};

pub(crate) fn resolve_load_config_path(path: Option<PathBuf>) -> Option<PathBuf> {
    if let Some(path) = path {
        return Some(expand_pathbuf(path));
    }

    if let Some(path) = env_config_path() {
        if path.exists() {
            return Some(path);
        }

        if let Some(home_path) = home_config_path()
            && home_path.exists()
        {
            return Some(home_path);
        }

        return Some(path);
    }

    home_config_path()
}

/// Create an inspectable config file on first interactive launch.
///
/// The file intentionally omits `api_key`; onboarding or `mimofan auth set`
/// writes that field after the user supplies a key.
pub fn ensure_config_file_exists(path: Option<PathBuf>) -> Result<Option<PathBuf>> {
    let config_path = path
        .map(expand_pathbuf)
        .or_else(default_config_path)
        .context("Failed to resolve config path: home directory not found.")?;
    if config_path.exists() {
        return Ok(None);
    }

    ensure_parent_dir(&config_path)?;
    let content = format!(
        r#"# mimofan Configuration
# Get your API key from https://platform.deepseek.com
# Save it with: mimofan auth set --provider deepseek

# Base URL (default: https://api.deepseek.com/beta)
# Set https://api.deepseek.com to opt out of beta features.
# base_url = "https://api.deepseek.com/beta"

# Default model
default_text_model = "{DEFAULT_TEXT_MODEL}"

# Thinking mode (DeepSeek V4 reasoning effort):
# "auto" | "off" | "low" | "medium" | "high" | "max"
# Shift+Tab in the TUI cycles between off / high / max.
reasoning_effort = "auto"

# Startup update check
[update]
check_for_updates = true
# update_uri = "https://internal.mirror.example/mimofan/releases/latest"
"#
    );
    write_config_file_secure(&config_path, &content)
        .with_context(|| format!("Failed to write config to {}", config_path.display()))?;
    Ok(Some(config_path))
}

fn normalize_model_for_provider(provider: ApiProvider, model: &str) -> Option<String> {
    if provider_passes_model_through(provider) {
        return None;
    }
    normalize_model_name_for_provider(provider, model)
}

pub(crate) fn provider_passes_model_through(provider: ApiProvider) -> bool {
    matches!(provider, ApiProvider::OpenAiCompatible)
}

fn default_base_url_for_provider(provider: ApiProvider) -> &'static str {
    mimofan_config::default_base_url_for_provider(
        provider
            .kind()
            .expect("ApiProvider always maps to a ProviderKind"),
    )
}

fn base_url_is_custom_for_provider(provider: ApiProvider, base_url: &str) -> bool {
    normalize_base_url(base_url) != normalize_base_url(default_base_url_for_provider(provider))
}

fn provider_preserves_custom_base_url_model(provider: ApiProvider, base_url: &str) -> bool {
    base_url_is_custom_for_provider(provider, base_url)
}

fn base_url_uses_local_host(base_url: &str) -> bool {
    let Some(host) = base_url_host(base_url) else {
        return false;
    };
    let host = host.trim_matches(['[', ']']).to_ascii_lowercase();
    if matches!(host.as_str(), "localhost" | "0.0.0.0") {
        return true;
    }
    host.parse::<std::net::IpAddr>()
        .is_ok_and(|addr| addr.is_loopback() || addr.is_unspecified())
}

fn base_url_host(base_url: &str) -> Option<&str> {
    let without_scheme = base_url
        .split_once("://")
        .map_or(base_url, |(_, rest)| rest);
    let authority = without_scheme.split('/').next()?.rsplit('@').next()?;
    if let Some(rest) = authority.strip_prefix('[') {
        return rest.split_once(']').map(|(host, _)| host);
    }
    authority.split(':').next().filter(|host| !host.is_empty())
}

fn normalize_base_url(base: &str) -> String {
    let trimmed = base.trim_end_matches('/');
    let deepseek_domains = ["api.deepseek.com", "api.deepseeki.com"];
    if deepseek_domains
        .iter()
        .any(|domain| trimmed.contains(domain))
    {
        return trimmed.trim_end_matches("/v1").to_string();
    }
    trimmed.to_string()
}

fn parse_http_headers(raw: &str) -> Result<HashMap<String, String>> {
    let mut headers = HashMap::new();
    for pair in raw.trim().split(',') {
        let pair = pair.trim();
        if pair.is_empty() {
            continue;
        }
        let Some((name, value)) = pair.split_once('=') else {
            anyhow::bail!("invalid header pair '{pair}', expected name=value");
        };
        let name = name.trim();
        let value = value.trim();
        if name.is_empty() {
            anyhow::bail!("header name cannot be empty");
        }
        if value.is_empty() {
            continue;
        }
        headers.insert(name.to_string(), value.to_string());
    }
    Ok(headers)
}

fn apply_profile(config: ConfigFile, profile: Option<&str>) -> Result<Config> {
    if let Some(profile_name) = profile {
        let profiles = config.profiles.as_ref();
        match profiles.and_then(|profiles| profiles.get(profile_name)) {
            Some(override_cfg) => Ok(override_cfg.clone()),
            None => {
                let available = profiles
                    .map(|profiles| {
                        let mut keys = profiles.keys().cloned().collect::<Vec<_>>();
                        keys.sort();
                        if keys.is_empty() {
                            "none".to_string()
                        } else {
                            keys.join(", ")
                        }
                    })
                    .unwrap_or_else(|| "none".to_string());
                anyhow::bail!("Profile '{profile_name}' not found. Available profiles: {available}")
            }
        }
    } else {
        Ok(config.base)
    }
}

fn merge_config(base: Config, override_cfg: Config) -> Config {
    Config {
        provider: override_cfg.provider.or(base.provider),
        api_key: override_cfg.api_key.or(base.api_key),
        base_url: override_cfg.base_url.or(base.base_url),
        http_headers: override_cfg.http_headers.or(base.http_headers),
        default_text_model: override_cfg.default_text_model.or(base.default_text_model),
        auth_mode: override_cfg.auth_mode.or(base.auth_mode),
        reasoning_effort: override_cfg.reasoning_effort.or(base.reasoning_effort),
        tools_file: override_cfg.tools_file.or(base.tools_file),
        tools: override_cfg.tools.or(base.tools),
        skills_dir: override_cfg.skills_dir.or(base.skills_dir),
        mcp_config_path: override_cfg.mcp_config_path.or(base.mcp_config_path),
        mcp_oauth_callback_port: override_cfg
            .mcp_oauth_callback_port
            .or(base.mcp_oauth_callback_port),
        mcp_oauth_callback_url: override_cfg
            .mcp_oauth_callback_url
            .or(base.mcp_oauth_callback_url),
        notes_path: override_cfg.notes_path.or(base.notes_path),
        memory_path: override_cfg.memory_path.or(base.memory_path),
        memory_dir: override_cfg.memory_dir.or(base.memory_dir),
        vision_model: override_cfg.vision_model.or(base.vision_model),
        // #454: user-owned overlays such as profiles and managed config may
        // replace the instruction array. Project-scope config is filtered in
        // main.rs and cannot set instruction paths.
        instructions: override_cfg.instructions.or(base.instructions),
        allow_shell: override_cfg.allow_shell.or(base.allow_shell),
        prompt_suggestion: override_cfg.prompt_suggestion.or(base.prompt_suggestion),
        yolo: override_cfg.yolo.or(base.yolo),
        verbosity: override_cfg.verbosity.or(base.verbosity),
        approval_policy: override_cfg.approval_policy.or(base.approval_policy),
        sandbox_mode: override_cfg.sandbox_mode.or(base.sandbox_mode),
        fallback_providers: if override_cfg.fallback_providers.is_empty() {
            base.fallback_providers
        } else {
            override_cfg.fallback_providers
        },
        sandbox_backend: override_cfg.sandbox_backend.or(base.sandbox_backend),
        sandbox_url: override_cfg.sandbox_url.or(base.sandbox_url),
        sandbox_api_key: override_cfg.sandbox_api_key.or(base.sandbox_api_key),
        prefer_bwrap: override_cfg.prefer_bwrap.or(base.prefer_bwrap),
        managed_config_path: override_cfg
            .managed_config_path
            .or(base.managed_config_path),
        requirements_path: override_cfg.requirements_path.or(base.requirements_path),
        max_subagents: override_cfg.max_subagents.or(base.max_subagents),
        retry: override_cfg.retry.or(base.retry),
        auto_review: override_cfg.auto_review.or(base.auto_review),
        tui: override_cfg.tui.or(base.tui),
        hooks: override_cfg.hooks.or(base.hooks),
        providers: merge_providers(base.providers, override_cfg.providers),
        features: merge_features(base.features, override_cfg.features),
        notifications: override_cfg.notifications.or(base.notifications),
        network: override_cfg.network.or(base.network),
        skills: merge_skills_config(base.skills, override_cfg.skills),
        snapshots: override_cfg.snapshots.or(base.snapshots),
        search: override_cfg.search.or(base.search),
        memory: override_cfg.memory.or(base.memory),
        speech: override_cfg.speech.or(base.speech),
        auto: override_cfg.auto.or(base.auto),
        hotbar: override_cfg.hotbar.or(base.hotbar),
        update: override_cfg.update.or(base.update),
        lsp: override_cfg.lsp.or(base.lsp),
        context: ContextConfig {
            enabled: override_cfg.context.enabled.or(base.context.enabled),
            project_pack: override_cfg
                .context
                .project_pack
                .or(base.context.project_pack),
            verbatim_window_turns: override_cfg
                .context
                .verbatim_window_turns
                .or(base.context.verbatim_window_turns),
            l1_threshold: override_cfg
                .context
                .l1_threshold
                .or(base.context.l1_threshold),
            l2_threshold: override_cfg
                .context
                .l2_threshold
                .or(base.context.l2_threshold),
            l3_threshold: override_cfg
                .context
                .l3_threshold
                .or(base.context.l3_threshold),
            seam_model: override_cfg.context.seam_model.or(base.context.seam_model),
        },
        fleet: override_cfg.fleet.or(base.fleet),
        subagents: override_cfg.subagents.or(base.subagents),
        limits: override_cfg.limits.or(base.limits),
        strict_tool_mode: override_cfg.strict_tool_mode.or(base.strict_tool_mode),
        runtime_api: override_cfg.runtime_api.or(base.runtime_api),
        workshop: override_cfg.workshop.or(base.workshop),
        exec_policy_engine: override_cfg.exec_policy_engine,
    }
}

fn load_sibling_exec_policy_engine(config_path: Option<&Path>) -> Result<ExecPolicyEngine> {
    let Some(config_path) = config_path else {
        return Ok(ExecPolicyEngine::new(Vec::new(), Vec::new()));
    };
    let permissions_path = mimofan_config::permissions_path_for_config_path(config_path);
    if !permissions_path.exists() {
        return Ok(ExecPolicyEngine::new(Vec::new(), Vec::new()));
    }

    let raw = fs::read_to_string(&permissions_path).with_context(|| {
        format!(
            "Failed to read permissions file: {}",
            permissions_path.display()
        )
    })?;
    let permissions: mimofan_config::PermissionsToml = toml::from_str(&raw).with_context(|| {
        format!(
            "Failed to parse permissions file: {}",
            permissions_path.display()
        )
    })?;
    if permissions.is_empty() {
        Ok(ExecPolicyEngine::new(Vec::new(), Vec::new()))
    } else {
        Ok(ExecPolicyEngine::with_rulesets(vec![permissions.ruleset()]))
    }
}

fn merge_skills_config(
    base: Option<SkillsConfig>,
    override_cfg: Option<SkillsConfig>,
) -> Option<SkillsConfig> {
    match (base, override_cfg) {
        (None, None) => None,
        (Some(base), None) => Some(base),
        (None, Some(override_cfg)) => Some(override_cfg),
        (Some(base), Some(override_cfg)) => Some(SkillsConfig {
            registry_url: override_cfg.registry_url.or(base.registry_url),
            max_install_size_bytes: override_cfg
                .max_install_size_bytes
                .or(base.max_install_size_bytes),
            scan_mimofan_only: override_cfg.scan_mimofan_only.or(base.scan_mimofan_only),
        }),
    }
}

fn merge_provider_config(base: ProviderConfig, override_cfg: ProviderConfig) -> ProviderConfig {
    ProviderConfig {
        api_key: override_cfg.api_key.or(base.api_key),
        base_url: override_cfg.base_url.or(base.base_url),
        model: override_cfg.model.or(base.model),
        mode: override_cfg.mode.or(base.mode),
        auth_mode: override_cfg.auth_mode.or(base.auth_mode),
        insecure_skip_tls_verify: override_cfg
            .insecure_skip_tls_verify
            .or(base.insecure_skip_tls_verify),
        http_headers: override_cfg.http_headers.or(base.http_headers),
        path_suffix: override_cfg.path_suffix.or(base.path_suffix),
        reasoning_stream_style: override_cfg
            .reasoning_stream_style
            .or(base.reasoning_stream_style),
        auth: override_cfg.auth.or(base.auth),
        kind: override_cfg.kind.or(base.kind),
        api_key_env: override_cfg.api_key_env.or(base.api_key_env),
    }
}

/// Merge the per-name custom provider maps (#1519): the union of both key sets,
/// with each shared key deep-merged via [`merge_provider_config`] (override
/// wins field-by-field). Keys present in only one map are carried through as-is.
fn merge_custom_providers(
    mut base: HashMap<String, ProviderConfig>,
    override_cfg: HashMap<String, ProviderConfig>,
) -> HashMap<String, ProviderConfig> {
    for (name, entry) in override_cfg {
        let merged = match base.remove(&name) {
            Some(base_entry) => merge_provider_config(base_entry, entry),
            None => entry,
        };
        base.insert(name, merged);
    }
    base
}

fn merge_providers(
    base: Option<ProvidersConfig>,
    override_cfg: Option<ProvidersConfig>,
) -> Option<ProvidersConfig> {
    match (base, override_cfg) {
        (None, None) => None,
        (Some(base), None) => Some(base),
        (None, Some(override_cfg)) => Some(override_cfg),
        (Some(base), Some(override_cfg)) => Some(ProvidersConfig {
            openai_compatible: merge_provider_config(
                base.openai_compatible,
                override_cfg.openai_compatible,
            ),
            anthropic_compatible: merge_provider_config(
                base.anthropic_compatible,
                override_cfg.anthropic_compatible,
            ),
            gemini_compatible: merge_provider_config(
                base.gemini_compatible,
                override_cfg.gemini_compatible,
            ),
            custom: merge_custom_providers(base.custom, override_cfg.custom),
        }),
    }
}

fn load_single_config_file(path: &Path) -> Result<Config> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file: {}", path.display()))?;
    let parsed: ConfigFile = toml::from_str(&contents)
        .with_context(|| format!("Failed to parse config file: {}", path.display()))?;
    Ok(parsed.base)
}

/// Build a one-line warning when top-level-only keys are nested under a section
/// mimofan does not define (`[general]` / `[sandbox]`). TOML silently drops
/// those keys, so e.g. `[general]\nallow_shell = true` never takes effect and
/// the shell tools (`exec_shell`, `task_shell_start`, …) are absent from the
/// catalog with no explanation. Returns `None` when nothing is misplaced.
///
/// This is the exact confusion behind #2589: `allow_shell` and `sandbox_mode`
/// belong at the top of the file, above any `[section]` header.
fn warn_on_misplaced_top_level_keys(raw: &str) -> Option<String> {
    let doc = toml::from_str::<toml::Value>(raw).ok()?;
    // Sections mimofan does not recognize but users nest settings under.
    const UNKNOWN_SECTIONS: &[&str] = &["general", "sandbox"];
    // Keys that are only ever read from the top level of the config.
    const TOP_LEVEL_KEYS: &[&str] = &[
        "allow_shell",
        "sandbox_mode",
        "approval_policy",
        "verbosity",
    ];

    let mut hits: Vec<String> = Vec::new();
    for section in UNKNOWN_SECTIONS {
        let Some(table) = doc.get(*section).and_then(toml::Value::as_table) else {
            continue;
        };
        for key in TOP_LEVEL_KEYS {
            if table.contains_key(*key) {
                hits.push(format!("`{section}.{key}`"));
            }
        }
    }
    if hits.is_empty() {
        return None;
    }
    Some(format!(
        "Ignoring {} — mimofan has no `[general]` or `[sandbox]` section, so these \
         keys are silently dropped. Move them to the TOP of the config file (above any \
         `[section]` header), e.g. `allow_shell = true`. Until then, shell tools stay \
         disabled. (#2589)",
        hits.join(", ")
    ))
}

fn apply_managed_overrides(config: &mut Config) -> Result<()> {
    let path = config
        .managed_config_path
        .as_deref()
        .map(expand_path)
        .or_else(default_managed_config_path);
    let Some(path) = path else {
        return Ok(());
    };
    if !path.exists() {
        return Ok(());
    }
    let managed = load_single_config_file(&path)?;
    *config = merge_config(config.clone(), managed);
    Ok(())
}

fn apply_requirements(config: &mut Config) -> Result<()> {
    let path = config
        .requirements_path
        .as_deref()
        .map(expand_path)
        .or_else(default_requirements_path);
    let Some(path) = path else {
        return Ok(());
    };
    if !path.exists() {
        return Ok(());
    }
    let contents = fs::read_to_string(&path)
        .with_context(|| format!("Failed to read requirements file: {}", path.display()))?;
    let requirements: RequirementsFile = toml::from_str(&contents)
        .with_context(|| format!("Failed to parse requirements file: {}", path.display()))?;

    if !requirements.allowed_approval_policies.is_empty()
        && let Some(policy) = config.approval_policy.as_ref()
    {
        let policy = policy.to_ascii_lowercase();
        if !requirements
            .allowed_approval_policies
            .iter()
            .any(|p| p.eq_ignore_ascii_case(&policy))
        {
            anyhow::bail!(
                "approval_policy '{policy}' is not allowed by requirements ({})",
                requirements.allowed_approval_policies.join(", ")
            );
        }
    }
    if !requirements.allowed_sandbox_modes.is_empty()
        && let Some(mode) = config.sandbox_mode.as_ref()
    {
        let mode = mode.to_ascii_lowercase();
        if !requirements
            .allowed_sandbox_modes
            .iter()
            .any(|m| m.eq_ignore_ascii_case(&mode))
        {
            anyhow::bail!(
                "sandbox_mode '{mode}' is not allowed by requirements ({})",
                requirements.allowed_sandbox_modes.join(", ")
            );
        }
    }

    Ok(())
}

fn merge_features(
    base: Option<FeaturesToml>,
    override_cfg: Option<FeaturesToml>,
) -> Option<FeaturesToml> {
    match (base, override_cfg) {
        (None, None) => None,
        (Some(mut base), Some(override_cfg)) => {
            for (key, value) in override_cfg.entries {
                base.entries.insert(key, value);
            }
            Some(base)
        }
        (Some(base), None) => Some(base),
        (None, Some(override_cfg)) => Some(override_cfg),
    }
}

pub fn ensure_parent_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
        #[cfg(unix)]
        {
            // Tighten group/other bits on the parent dir as a hardening pass.
            // The dir lives under the user's home, so the chmod is best-effort:
            // filesystems that don't accept Unix permission bits (Docker
            // bind-mounts of NTFS, network shares, FAT, certain CI volumes —
            // see #897) return EPERM/ENOTSUP. The dir already exists by the
            // time we get here, so failing the whole save just because we
            // couldn't tighten perms strands the user mid-onboarding. Warn
            // loudly so a security-sensitive operator can still notice via
            // `RUST_LOG=warn`, then continue.
            if let Ok(meta) = fs::metadata(parent) {
                let mode = meta.permissions().mode();
                if mode & 0o077 != 0 {
                    let mut perms = meta.permissions();
                    perms.set_mode(mode & !0o077);
                    if let Err(err) = fs::set_permissions(parent, perms) {
                        tracing::warn!(
                            target: "mimofan::config",
                            path = %parent.display(),
                            error = %err,
                            "could not tighten parent dir permissions; \
                             filesystem may not support Unix chmod \
                             (Docker bind-mount, NTFS, network share). \
                             Continuing — the file will still be written."
                        );
                    }
                }
            }
        }
    }
    Ok(())
}

/// Write content to a config file with restrictive permissions (owner-only read/write).
/// On Unix this sets mode 0o600 before writing.
fn write_config_file_secure(path: &Path, content: &str) -> Result<()> {
    #[cfg(unix)]
    {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(content.as_bytes())?;
        // The file was already opened with mode 0o600; the explicit
        // set_permissions re-asserts that on filesystems where mode-at-open
        // didn't take effect (or where the file already existed with broader
        // bits). Filesystems that don't accept Unix chmod at all (Docker
        // bind-mounts of NTFS, network shares — #897) return EPERM. Treat
        // that as a warning rather than failing the whole save: the file
        // contents are written, and on Windows/macOS hosts the parent file
        // system's native ACL model is doing the access control.
        if let Err(err) = file.set_permissions(fs::Permissions::from_mode(0o600)) {
            tracing::warn!(
                target: "mimofan::config",
                path = %path.display(),
                error = %err,
                "could not enforce 0o600 on config file; filesystem may \
                 not support Unix chmod. File contents written; rely on \
                 host ACLs for access control."
            );
        }
    }
    #[cfg(not(unix))]
    {
        fs::write(path, content)?;
    }
    Ok(())
}
