//! Type definitions for the TUI application.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::time::Instant;

use ratatui::layout::Rect;
use serde_json::Value;

use mimofan_config::{
    ProviderChain, catalog::ProviderCatalogCache, route::RouteLimits,
};

use crate::artifacts::ArtifactRecord;
use crate::config::{ApiProvider, Config};
use crate::hooks::HookExecutor;
use crate::localization::Locale;
use crate::models::{Message, SystemPrompt};
use crate::palette::{self, UiTheme};
use crate::pricing::CostCurrency;
use crate::session_manager::SessionContextReference;
use crate::tools::plan::SharedPlanState;
use crate::tools::spec::RuntimeToolServices;
use crate::tools::subagent::SubAgentResult;
use crate::tools::todo::SharedTodoList;
use crate::tui::active_cell::ActiveCell;
use crate::tui::approval::ApprovalMode;
use crate::tui::clipboard::ClipboardHandler;
use crate::tui::composer::ComposerState;
use crate::tui::history::HistoryCell;
use crate::tui::history_search::HuntState;
use crate::tui::hotbar::HotbarActionRegistry;
use crate::tui::sidebar::SidebarWorkSummary;
use crate::tui::state::{SessionState, SidebarHoverState};
use crate::tui::streaming::StreamingState;
use crate::tui::tool_collapse::ToolCollapseMode;
use crate::tui::viewport::ViewportState;
use crate::tui::views::ViewStack;

// === Enums ===

/// State machine for onboarding new users.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnboardingState {
    Welcome,
    /// Pick the UI locale before any other config decisions (#566).
    /// Defaults to auto-detection from `LC_ALL` / `LANG`; explicit picks
    /// land in the persisted settings.json via `Settings::set("locale", …)`.
    Language,
    ApiKey,
    TrustDirectory,
    Tips,
    None,
}

/// Supported application modes for the TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppMode {
    Agent,
    Yolo,
    Plan,
}

/// Reasoning-effort tier, mirrored across DeepSeek and Codex effort pickers.
///
/// The config file accepts all five string values for forward-compat with
/// providers that expose the full spectrum; DeepSeek currently collapses
/// `Low`/`Medium` → `high`. OpenAI Codex normalizes inherited DeepSeek-only
/// `Off` to `Low` and displays/sends `Max` as `xhigh` at the provider
/// boundary. The default keyboard cycler walks the three DeepSeek-distinct
/// tiers: `Off` → `High` → `Max` → `Off`; provider-aware callers should use
/// [`ReasoningEffort::cycle_next_for_provider`].
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ReasoningEffort {
    Off,
    Low,
    Medium,
    High,
    Auto,
    #[default]
    Max,
}

/// Sidebar content focus mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarFocus {
    Auto,
    Pinned,
    Tasks,
    Agents,
    Context,
    Hidden,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComposerDensity {
    Compact,
    Comfortable,
    Spacious,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptSpacing {
    Compact,
    Comfortable,
    Spacious,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusToastLevel {
    Info,
    Success,
    Warning,
    Error,
}

/// How a freshly-typed user input should be sent.
///
/// Picked by [`App::decide_submit_disposition`] when the user hits Enter on a
/// non-empty composer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmitDisposition {
    /// Engine idle and online: send immediately.
    Immediate,
    /// Park on `queued_messages` (offline, or engine busy — #382).
    Queue,
    /// Explicit steer via Ctrl+Enter (#382). Not returned by `decide_submit_disposition`.
    Steer,
    /// Park on `queued_messages` for dispatch after TurnComplete.
    /// Legacy path; #382 unified busy states under `Queue`.
    QueueFollowUp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskPanelEntryKind {
    Background,
    ModelReasoning,
}

// === Enum impls ===

impl AppMode {
    /// Human-readable label used in status messages and hook contexts.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Agent => "Agent",
            Self::Yolo => "YOLO",
            Self::Plan => "Plan",
        }
    }

    /// Display name for mode switching status messages.
    #[must_use]
    pub fn display_name(self) -> &'static str {
        self.label()
    }

    /// Canonical setting string for config persistence.
    #[must_use]
    pub fn as_setting(self) -> &'static str {
        match self {
            Self::Agent => "agent",
            Self::Yolo => "yolo",
            Self::Plan => "plan",
        }
    }

    /// Parse from a setting string, defaulting to Agent on unrecognized values.
    #[must_use]
    pub fn from_setting(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "plan" => Self::Plan,
            "yolo" => Self::Yolo,
            _ => Self::Agent,
        }
    }

    /// Parse from a user-supplied argument with aliases. Returns `None` on
    /// unrecognized input.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "agent" | "edit" | "normal" | "1" => Some(Self::Agent),
            "plan" | "2" => Some(Self::Plan),
            "yolo" | "3" => Some(Self::Yolo),
            _ => None,
        }
    }

    /// All modes in canonical cycle order.
    pub const CHOICES: &'static [AppMode] = &[Self::Agent, Self::Plan, Self::Yolo];

    /// Next mode in the cycle: Agent -> Yolo -> Plan -> Agent.
    #[must_use]
    pub fn next(self) -> Self {
        match self {
            Self::Agent => Self::Yolo,
            Self::Yolo => Self::Plan,
            Self::Plan => Self::Agent,
        }
    }

    /// Previous mode in the cycle: Agent -> Plan -> Yolo -> Agent.
    #[must_use]
    pub fn previous(self) -> Self {
        match self {
            Self::Agent => Self::Plan,
            Self::Plan => Self::Yolo,
            Self::Yolo => Self::Agent,
        }
    }

    /// Numeric key for the mode picker (1, 2, 3).
    #[must_use]
    pub fn number(self) -> char {
        match self {
            Self::Agent => '1',
            Self::Plan => '2',
            Self::Yolo => '3',
        }
    }

    /// Alias for `label()` — used by the engine status events.
    #[must_use]
    pub fn description(self) -> &'static str {
        self.label()
    }

    /// Locale-aware display name for the mode picker.
    #[must_use]
    pub fn display_name_localized(self, _locale: Locale) -> &'static str {
        self.label()
    }

    /// Locale-aware hint shown alongside each mode in the picker.
    #[must_use]
    pub fn picker_hint_localized(self, _locale: Locale) -> &'static str {
        match self {
            Self::Agent => "normal tools",
            Self::Plan => "read-only plan",
            Self::Yolo => "auto-approve",
        }
    }
}

impl ReasoningEffort {
    /// Parse from a raw config setting string (provider-agnostic). Recognizes
    /// common aliases; unrecognised values fall through to `Max`.
    #[must_use]
    pub fn from_setting(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "off" | "disabled" | "none" | "false" => Self::Off,
            "low" | "minimal" => Self::Low,
            "medium" | "mid" => Self::Medium,
            "high" => Self::High,
            "auto" | "automatic" => Self::Auto,
            _ => Self::Max,
        }
    }

    /// Canonical setting string (provider-agnostic, DeepSeek-format).
    #[must_use]
    pub fn as_setting(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Auto => "auto",
            Self::Max => "max",
        }
    }

    /// Parse from a raw config value with provider-specific aliases
    /// (e.g. `xhigh`/`ultracode` for DeepSeek).
    #[must_use]
    pub fn from_setting_for_provider(value: &str, _provider: ApiProvider) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "off" | "disabled" | "none" | "false" => Self::Off,
            "low" | "minimal" => Self::Low,
            "medium" | "mid" => Self::Medium,
            "high" => Self::High,
            "auto" | "automatic" => Self::Auto,
            _ => Self::Max,
        }
    }

    /// Canonical setting string for the given provider's config format.
    #[must_use]
    pub fn as_setting_for_provider(self, _provider: ApiProvider) -> &'static str {
        self.as_setting()
    }

    /// Normalize effort for a specific provider. OpenAI-compatible providers
    /// keep all tiers; other compat modes collapse `Low`/`Medium` into `High`
    /// since they only distinguish on/off/maximum.
    #[must_use]
    pub fn normalize_for_provider(self, provider: ApiProvider) -> Self {
        if provider == ApiProvider::OpenAiCompatible {
            return self;
        }
        match self {
            Self::Low | Self::Medium => Self::High,
            other => other,
        }
    }

    /// Provider-specific value sent on the wire (e.g. `"off"`, `"high"`,
    /// `"max"` for DeepSeek; `"1"`–`"5"` for Anthropic-style).
    /// Returns `None` for `Auto` since it has no direct wire representation.
    #[must_use]
    pub fn api_value_for_provider(self, provider: ApiProvider) -> Option<&'static str> {
        match provider {
            ApiProvider::OpenAiCompatible => Some(match self {
                Self::Off => "off",
                Self::Low => "low",
                Self::Medium => "medium",
                Self::High => "high",
                Self::Auto => return None,
                Self::Max => "max",
            }),
            _ => Some(match self {
                Self::Off => "low",
                Self::Low => "medium",
                Self::Medium => "high",
                Self::High => "high",
                Self::Auto => return None,
                Self::Max => "max",
            }),
        }
    }

    /// Human-readable label for the given provider (used in status chips
    /// and picker UIs).
    #[must_use]
    pub fn display_label_for_provider(self, provider: ApiProvider) -> &'static str {
        match provider {
            ApiProvider::OpenAiCompatible => match self {
                Self::Off => "Off",
                Self::Low => "Low",
                Self::Medium => "Medium",
                Self::High => "High",
                Self::Auto => "Auto",
                Self::Max => "Max",
            },
            _ => match self {
                Self::Off => "Off",
                Self::Low => "Low",
                Self::Medium => "Medium",
                Self::High => "High",
                Self::Auto => "Auto",
                Self::Max => "Max",
            },
        }
    }

    /// Cycle to the next effort tier supported by the given provider.
    ///
    /// OpenAI-compatible: Off -> High -> Max -> Off
    /// Other providers: Off -> High -> Max -> Off
    #[must_use]
    pub fn cycle_next_for_provider(self, _provider: ApiProvider) -> Self {
        match self {
            Self::Off => Self::High,
            Self::High => Self::Max,
            Self::Max => Self::Off,
            // Low/Medium/Auto are not in the primary cycle — jump to Off.
            _ => Self::Off,
        }
    }
}

impl SidebarFocus {
    /// Canonical setting string for config persistence.
    #[must_use]
    pub fn as_setting(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Pinned => "pinned",
            Self::Tasks => "tasks",
            Self::Agents => "agents",
            Self::Context => "context",
            Self::Hidden => "hidden",
        }
    }

    /// Parse from a setting string, defaulting to Auto on unrecognized values.
    #[must_use]
    pub fn from_setting(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "pinned" | "visible" | "show" | "on" | "work" | "plan" | "todos" => Self::Pinned,
            "tasks" => Self::Tasks,
            "agents" | "subagents" | "sub-agents" => Self::Agents,
            "context" | "session" => Self::Context,
            "hidden" | "hide" | "closed" | "off" | "none" => Self::Hidden,
            _ => Self::Auto,
        }
    }
}

impl ComposerDensity {
    /// Parse from a setting string, defaulting to Comfortable.
    #[must_use]
    pub fn from_setting(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "compact" | "tight" => Self::Compact,
            "comfortable" | "default" | "normal" => Self::Comfortable,
            "spacious" | "loose" => Self::Spacious,
            _ => Self::Comfortable,
        }
    }

    /// Canonical setting string for config persistence.
    #[must_use]
    pub fn as_setting(self) -> &'static str {
        match self {
            Self::Compact => "compact",
            Self::Comfortable => "comfortable",
            Self::Spacious => "spacious",
        }
    }
}

impl TranscriptSpacing {
    /// Parse from a setting string, defaulting to Comfortable.
    #[must_use]
    pub fn from_setting(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "compact" | "tight" => Self::Compact,
            "comfortable" | "default" | "normal" => Self::Comfortable,
            "spacious" | "loose" => Self::Spacious,
            _ => Self::Comfortable,
        }
    }

    /// Canonical setting string for config persistence.
    #[must_use]
    pub fn as_setting(self) -> &'static str {
        match self {
            Self::Compact => "compact",
            Self::Comfortable => "comfortable",
            Self::Spacious => "spacious",
        }
    }
}

// === Structs ===

/// One row in the per-turn cache-telemetry ring (`/cache` debug surface, #263).
#[derive(Debug, Clone)]
pub struct TurnCacheRecord {
    /// Provider-reported total input tokens for the turn (cache-hit +
    ///   cache-miss + uncategorized). Useful for sanity-checking that hits +
    ///   misses sum back to roughly the prompt size.
    pub input_tokens: u32,
    /// Provider-reported output tokens.
    pub output_tokens: u32,
    /// `prompt_cache_hit_tokens` from DeepSeek's usage payload. `None` when
    ///   the model in use does not report cache telemetry (see
    ///   `Capabilities::cache_telemetry_supported`).
    pub cache_hit_tokens: Option<u32>,
    /// `prompt_cache_miss_tokens`. `None` when the provider did not report it
    ///   — in that case the `/cache` formatter infers the miss as
    ///   `input_tokens − cache_hit_tokens`.
    pub cache_miss_tokens: Option<u32>,
    /// Approximate tokens spent re-sending prior `reasoning_content` on
    ///   V4-thinking tool-calling turns (chars/3 heuristic). Helps separate
    ///   cache misses caused by reasoning-replay churn from misses caused by
    ///   real prefix instability.
    pub reasoning_replay_tokens: Option<u32>,
    /// Local timestamp the turn telemetry was recorded.
    pub recorded_at: Instant,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentProgressMeta {
    pub parent_run_id: Option<String>,
    pub spawn_depth: u32,
}

#[derive(Debug, Clone)]
pub struct StatusToast {
    pub text: String,
    pub level: StatusToastLevel,
    pub created_at: Instant,
    pub ttl_ms: Option<u64>,
}

impl StatusToast {
    #[must_use]
    pub fn new(text: impl Into<String>, level: StatusToastLevel, ttl_ms: Option<u64>) -> Self {
        Self {
            text: text.into(),
            level,
            created_at: Instant::now(),
            ttl_ms,
        }
    }

    #[must_use]
    pub fn is_expired(&self, now: Instant) -> bool {
        self.ttl_ms
            .is_some_and(|ttl| now.duration_since(self.created_at).as_millis() >= u128::from(ttl))
    }
}

/// Options for creating a new TUI instance.
#[derive(Debug, Clone)]
pub struct TuiOptions {
    pub model: String,
    pub workspace: PathBuf,
    pub config_path: Option<PathBuf>,
    pub config_profile: Option<String>,
    pub allow_shell: bool,
    pub use_alt_screen: bool,
    pub use_mouse_capture: bool,
    pub use_bracketed_paste: bool,
    pub max_subagents: usize,
    pub skills_dir: PathBuf,
    pub memory_dir: PathBuf,
    pub notes_path: PathBuf,
    pub mcp_config_path: PathBuf,
    pub use_memory: bool,
    pub start_in_agent_mode: bool,
    pub skip_onboarding: bool,
    pub yolo: bool,
    pub resume_session_id: Option<String>,
    pub initial_input: Option<InitialInput>,
}

/// Initial input mode for the TUI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InitialInput {
    /// Used by `mimofan pr <N>` (#451) to drop the model into a session
    /// with the PR context already typed so the user can edit before sending.
    Prefill(String),
    /// Pre-populate the composer, submit it once startup is ready, then keep
    /// the interactive session open for follow-up messages (#2370).
    Submit(String),
}

/// Durable Agent-era permission baseline that Plan/YOLO restore to (#3386).
///
/// Mode cycling used to be tangled with permission policy: each mode mutated
/// `allow_shell`/`trust_mode`/`approval_mode` directly and ad-hoc
/// `YoloRestoreState`/`PlanRestoreState` snapshots tried to put things back on
/// exit. That made it easy to leak YOLO's elevated authority into Agent.
///
/// Instead we keep one canonical baseline here — the permission surface the
/// user has chosen for Agent mode — and derive every mode's effective policy
/// from it via [`base_policy_for_mode`]. `set_mode` refreshes this from the
/// live fields whenever the user leaves Agent, so toggling shell/trust/approval
/// in Agent (wherever that happens in the UI) is captured before any transient
/// Plan/YOLO policy overwrites the live mirrors.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ModeSessionPrefs {
    pub(crate) agent_allow_shell: bool,
    pub(crate) agent_trust_mode: bool,
    pub(crate) agent_approval_mode: ApprovalMode,
}

/// The permission policy a given [`AppMode`] resolves to (#3386).
///
/// This is a pure projection of `(mode, prefs)` — see [`base_policy_for_mode`].
/// The App keeps `allow_shell`/`trust_mode`/`approval_mode`/`yolo` as derived
/// mirrors of these values so the rest of the crate can keep reading the plain
/// booleans without a type migration.
#[derive(Debug, Clone, Copy)]
pub(crate) struct EffectiveModePolicy {
    pub(crate) mode: AppMode,
    pub(crate) allow_shell: bool,
    pub(crate) trust_mode: bool,
    pub(crate) approval_mode: ApprovalMode,
    /// Whether tool calls auto-approve (YOLO authority). Mirrors `self.yolo`.
    pub(crate) auto_approve: bool,
}

/// Evidence collected during a turn for the post-turn receipt.
#[derive(Debug, Clone)]
pub struct ToolEvidence {
    pub tool_name: String,
    pub summary: String,
}

#[derive(Debug, Clone)]
pub(crate) struct PendingProviderSwitch {
    pub previous_provider: ApiProvider,
    pub previous_model: String,
    pub previous_model_ids_passthrough: bool,
    pub previous_route_limits: Option<RouteLimits>,
    pub previous_config: Config,
    pub previous_onboarding: OnboardingState,
    pub previous_onboarding_needs_api_key: bool,
    pub previous_api_key_env_only: bool,
}

/// Message queued while the engine is busy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedMessage {
    pub display: String,
    pub skill_instruction: Option<String>,
}

impl QueuedMessage {
    pub fn new(display: String, skill_instruction: Option<String>) -> Self {
        Self {
            display,
            skill_instruction,
        }
    }
}

/// Detailed tool payload attached to a history cell.
#[derive(Debug, Clone)]
pub struct ToolDetailRecord {
    pub tool_id: String,
    pub tool_name: String,
    pub input: Value,
    pub output: Option<String>,
}

/// Lightweight task view for sidebar rendering.
#[derive(Debug, Clone)]
pub struct TaskPanelEntry {
    pub id: String,
    pub status: String,
    pub prompt_summary: String,
    pub duration_ms: Option<u64>,
    pub kind: TaskPanelEntryKind,
    pub stale: bool,
    pub elapsed_since_output_ms: Option<u64>,
    pub owner_agent_id: Option<String>,
    pub owner_agent_name: Option<String>,
}

/// Global UI state for the TUI.
#[allow(clippy::struct_excessive_bools)]
pub struct App {
    pub mode: AppMode,
    /// Registered hotbar actions available for future slot config/render layers.
    pub hotbar_actions: HotbarActionRegistry,
    /// Composer sub-state (input, cursor, history, menus).
    pub composer: ComposerState,
    /// Viewport sub-state (scroll, cache, selection).
    pub viewport: ViewportState,
    /// Goal sub-state.
    pub hunt: HuntState,
    /// Session sub-state (cost, tokens, telemetry).
    pub session: SessionState,
    /// Active tool restriction from custom slash command frontmatter.
    /// `None` means the current turn may use the normal tool set.
    pub active_allowed_tools: Option<Vec<String>>,
    /// True when the active custom slash command opted into pause/resume.
    pub pausable: bool,
    /// True after Esc paused a pausable command and before it is resumed or cancelled.
    pub paused: bool,
    /// Saved custom-command objective while the command is paused.
    pub paused_quarry: Option<String>,
    pub history: Vec<HistoryCell>,
    pub history_version: u64,
    /// Per-cell revision counter, kept in lockstep with `history`.
    pub history_revisions: Vec<u64>,
    /// Monotonic counter used to issue fresh per-cell revisions.
    pub next_history_revision: u64,
    pub api_messages: Vec<Message>,
    pub is_loading: bool,
    /// Whether the once-per-turn provider-wait incident (#3095) has already
    /// been logged for the current turn.
    pub provider_wait_incident_logged: bool,
    /// Ghost-text follow-up suggestion shown in the composer when empty.
    /// Generated asynchronously after each completed turn; cleared on new input.
    pub prompt_suggestion: Option<String>,
    /// Monotonic turn counter for stale-suggestion protection. Incremented on
    /// each TurnStarted; background suggestion tasks capture the token and
    /// discard their result if the token no longer matches.
    pub prompt_suggestion_gen: std::sync::atomic::AtomicU64,
    /// Degraded connectivity mode; new user inputs are queued for later retry.
    pub offline_mode: bool,
    /// Whether an `EngineEvent::Error` has already been posted for the
    /// current turn. Suppresses the redundant "Turn failed:" status line
    /// that `TurnComplete { error: .. }` would otherwise emit on top of
    /// the in-transcript error cell.
    pub turn_error_posted: bool,
    /// Legacy status text sink retained for compatibility with existing call sites.
    pub status_message: Option<String>,
    /// Recent status toasts (ephemeral, newest at back).
    pub status_toasts: VecDeque<StatusToast>,
    /// Sticky status toast used for important warnings/errors.
    pub sticky_status: Option<StatusToast>,
    /// Last status text already promoted from `status_message` into toast state.
    pub last_status_message_seen: Option<String>,
    pub model: String,
    /// Persisted model selections by provider name. Loaded from settings so
    /// `/model` and the picker can surface saved provider-specific choices.
    pub provider_models: HashMap<String, String>,
    /// When true, the model is auto-selected based on request complexity
    /// rather than using a fixed model. The `/model auto` command sets this.
    /// `dispatch_user_message` calls `auto_model_heuristic` to resolve the
    /// effective model for each outbound message.
    pub auto_model: bool,
    /// Last concrete model chosen while `auto_model` is active.
    pub last_effective_model: Option<String>,
    /// Current API provider (mirrors `Config::api_provider`).
    /// Updated by `/provider` switches so the UI/commands can read the
    /// active backend without re-deriving it from the live config.
    pub api_provider: ApiProvider,
    /// Primary provider plus configured fallback providers for this session.
    pub provider_chain: Option<ProviderChain>,
    /// Per-provider auth/local readiness snapshot for the fallback chain (#2574).
    ///
    /// Captured at startup alongside `provider_chain` (where the live `Config` is
    /// in scope). `advance_fallback` consults it to skip chain entries that
    /// cannot serve a turn — hosted providers missing a key — while local
    /// providers (Ollama/vLLM/SGLang) are always ready. Stored as `(provider,
    /// ready)` pairs; lookups fall back to "ready" for providers not present so
    /// an unknown entry is tried rather than silently skipped.
    pub(crate) provider_readiness: Vec<(ApiProvider, bool)>,
    /// Human-readable description of the last provider fallback event.
    pub last_fallback_reason: Option<String>,
    /// True when the active provider/base URL accepts arbitrary model IDs
    /// verbatim rather than DeepSeek-only aliases.
    pub model_ids_passthrough: bool,
    /// Resolved provider/model route limits for the active runtime route.
    pub active_route_limits: Option<RouteLimits>,
    /// Pending provider transition for transactional rollback when the next
    /// auth failure indicates the new provider cannot be used.
    pub(crate) pending_provider_switch: Option<PendingProviderSwitch>,
    /// Shared, process-wide live catalog cache (global, multi-provider). The
    /// same `Arc` is handed to the `Engine` so a refresh from either side is
    /// visible to the provider picker and the model list (#3385).
    pub catalog_cache: std::sync::Arc<std::sync::Mutex<ProviderCatalogCache>>,
    /// Current reasoning-effort tier for DeepSeek thinking mode.
    /// Cycled via Shift+Tab; initialized from config at startup.
    pub reasoning_effort: ReasoningEffort,
    /// Last concrete thinking tier chosen while `reasoning_effort` is auto.
    pub last_effective_reasoning_effort: Option<ReasoningEffort>,
    /// Whether `/fast` mode is currently active.
    pub fast_mode_active: bool,
    /// Model saved before entering fast mode (for `/normal` restore).
    pub fast_saved_model: Option<String>,
    /// Reasoning effort saved before entering fast mode (for `/normal` restore).
    pub fast_saved_effort: Option<ReasoningEffort>,
    pub workspace: PathBuf,
    pub config_path: Option<PathBuf>,
    pub config_profile: Option<String>,
    pub mcp_config_path: PathBuf,
    pub skills_dir: PathBuf,
    pub skills_scan_mimofan_only: bool,
    /// Path to the categorized user-memory directory (`~/.mimofan/memory/`).
    /// Always populated; only consulted when `use_memory` is `true`.
    pub memory_dir: PathBuf,
    /// Whether the user-memory feature is enabled (#489). Mirrors
    /// `Config::memory_enabled()` at app boot. Used by the `# foo`
    /// composer interception, the `/memory` slash command, and tool
    /// registration for `remember`.
    pub use_memory: bool,
    pub use_alt_screen: bool,
    pub use_mouse_capture: bool,
    /// When true, plain Up/Down on an empty composer scroll the transcript
    /// instead of navigating input history.  Defaults to `true` when mouse
    /// capture is off: terminals that convert mouse-wheel events to arrow-key
    /// sequences (e.g. Windows CMD without `WT_SESSION`) get page-scrolling
    /// without any explicit config (#1443).
    pub composer_arrows_scroll: bool,
    /// Data-side cap for the `@`-mention popup. The renderer still limits the
    /// visible rows to available terminal height.
    pub mention_limit: usize,
    /// Maximum workspace depth for `@`-mention completion walks. `0` means
    /// unlimited depth.
    pub mention_depth: usize,
    /// `@`-mention completion behavior: fuzzy workspace search or deterministic
    /// directory browser.
    pub mention_behavior: String,
    /// Follow symbolic links during workspace file discovery walks.
    /// When `true`, symlinked directories are traversed, enabling
    /// multi-project workspaces.
    pub workspace_follow_symlinks: bool,
    pub use_bracketed_paste: bool,
    pub use_paste_burst_detection: bool,
    /// Set to `true` the first time a real `Event::Paste` arrives during a
    /// session. Once set, `handle_paste_burst_key` short-circuits — there's
    /// no point running the rapid-keypress heuristic on a terminal that
    /// already delivers paste-as-event correctly. Avoids paste-burst false
    /// positives on Ghostty / iTerm2 / WezTerm / Windows Terminal where
    /// fast typing or IME commits could otherwise be mis-classified as a
    /// paste burst (#1322 follow-up).
    pub bracketed_paste_seen: bool,
    pub system_prompt: Option<SystemPrompt>,
    pub auto_compact: bool,
    pub auto_compact_user_configured: bool,
    pub compact_threshold_percent: f64,
    pub calm_mode: bool,
    pub low_motion: bool,
    /// Pending #61 (animated working strip). Set from config but not read
    /// until the footer widget consumes it.
    pub fancy_animations: bool,
    /// Whether the renderer should wrap each frame in DEC mode 2026
    /// synchronized output. Resolved from `Settings::synchronized_output`
    /// at construction; `auto`/`on` → `true`, `off` → `false`. The Ptyxis
    /// auto-detect path in `Settings::apply_env_overrides` flips `auto`
    /// to `off` before App is built, so by the time we read this flag in
    /// the draw loop the decision is already made. See the
    /// `Settings::synchronized_output` doc for the user-facing knob.
    pub synchronized_output_enabled: bool,
    /// Header status-indicator chip mode. One of `"whale"` (default, cycles
    /// 🐳→🐋 frames keyed off `turn_started_at`), `"dots"` (geometric ◌
    /// frames), or `"off"` (chip hidden entirely). Loaded from settings;
    /// changed via `/config status_indicator <whale|dots|off>`.
    pub status_indicator: String,
    pub show_thinking: bool,
    pub verbose_transcript: bool,
    pub show_tool_details: bool,
    pub ui_locale: Locale,
    pub cost_currency: CostCurrency,
    /// Resolved cost upper-bound budget guard (#620). When inactive (no
    /// `[cost_budget]` config) this is a no-op and never emits alerts.
    pub cost_budget: crate::cost_budget::CostBudget,
    /// Per-calendar-day cost accrual backing `CostBudgetKind::Daily`. Reset
    /// automatically when the local date rolls over.
    pub daily_cost_usd: f64,
    /// Local date string (`YYYY-MM-DD`) for which `daily_cost_usd` is valid.
    pub daily_cost_date: String,
    /// Highest budget alert level already surfaced per ceiling, so the same
    /// threshold doesn't re-toast on every subsequent accrual.
    pub cost_budget_alerts: std::collections::HashMap<
        crate::cost_budget::CostBudgetKind,
        crate::cost_budget::CostBudgetLevel,
    >,
    pub composer_density: ComposerDensity,
    pub composer_border: bool,
    /// Voice input state — toggled by `/voice` and the voice hotbar action.
    pub voice_enabled: bool,
    /// Auto-send after transcription when the transcript ends with an
    /// explicit send instruction ("send it" / "发送"). Toggled by `/voice-send`.
    pub voice_send_enabled: bool,
    /// AI-assisted dictation that sees the current composer text.
    /// Toggled by `/voice-control`.
    pub voice_control_enabled: bool,
    pub transcript_spacing: TranscriptSpacing,
    pub sidebar_width: u16,
    pub sidebar_focus: SidebarFocus,
    /// Sidebar hover state for mouse tooltip support.
    pub sidebar_hover: SidebarHoverState,
    /// Current hover tooltip text, if any.
    pub sidebar_hover_tooltip: Option<String>,
    /// Last successfully rendered Work panel summary. Transient mutex misses
    /// should not wipe completed checklist/strategy state from the sidebar.
    pub(crate) cached_work_summary: Option<SidebarWorkSummary>,
    /// Last known mouse position for tooltip placement.
    pub last_mouse_pos: Option<(u16, u16)>,
    /// Whether the user is currently dragging the sidebar resize handle.
    pub sidebar_resizing: bool,
    /// Mouse column at the start of a sidebar-resize drag.
    pub sidebar_resize_anchor_x: u16,
    /// Sidebar width in columns at the start of a sidebar-resize drag.
    pub sidebar_resize_anchor_width: u16,
    /// Last sidebar area rendered (for mouse hit-testing the resize handle).
    pub last_sidebar_area: Option<Rect>,
    /// Last total chat/sidebar width considered for sidebar rendering.
    pub last_sidebar_host_width: Option<u16>,
    /// Handle rect painted on the left edge of the sidebar (1 col).
    pub last_sidebar_handle_area: Option<Rect>,
    /// Total horizontal space (chat + sidebar) used to compute the percentage
    /// during sidebar resize drag.
    pub sidebar_resize_total_width: u16,
    /// Sidebar width changed during this drag and needs persistence.
    pub sidebar_width_dirty: bool,
    /// Sidebar focus/hidden state changed and needs persistence.
    pub sidebar_focus_dirty: bool,
    /// Whether the session-context panel is enabled (#504).
    pub context_panel: bool,
    /// Minimum number of consecutive safe tool cells needed for auto-collapse.
    pub tool_collapse_threshold: usize,
    /// Tool runs the user explicitly expanded. Stores original history indices.
    pub expanded_tool_runs: HashSet<usize>,
    /// Current dense tool-run collapse behavior.
    pub tool_collapse_mode: ToolCollapseMode,
    /// File-tree pane state. `None` when hidden; `Some` when visible.
    pub file_tree: Option<crate::tui::file_tree::FileTreeState>,
    /// Whether the file-tree pane was actually rendered in the last frame.
    /// Set false when the terminal is too narrow to show the tree.
    pub file_tree_visible: bool,
    pub compact_threshold: usize,
    /// Persistent `# Compact Instructions` from the project's AGENTS.md.
    ///
    /// Resolved once at startup because `compaction_config()` runs on hot UI
    /// paths and re-reading AGENTS.md there would cost a syscall per frame.
    pub compact_instructions: Option<String>,
    pub max_input_history: usize,
    pub allow_shell: bool,
    pub verbosity: Option<String>,
    pub max_subagents: usize,
    /// Per-SSE-chunk idle timeout for streamed turns, in seconds.
    pub stream_chunk_timeout_secs: u64,
    /// Cached sub-agent snapshots for UI views.
    pub subagent_cache: Vec<SubAgentResult>,
    /// First time this TUI observed each terminal sub-agent card.
    pub subagent_terminal_seen_at: HashMap<String, Instant>,
    /// Last known per-agent progress text for running sub-agents.
    pub agent_progress: HashMap<String, String>,
    /// Parent/depth metadata for live progress-only sub-agent rows.
    pub agent_progress_meta: HashMap<String, AgentProgressMeta>,
    /// In-transcript sub-agent card index by `agent_id` (issue #128).
    /// Maps each live sub-agent to the `HistoryCell::SubAgent` it renders
    /// into, so successive mailbox envelopes mutate the same cell rather
    /// than spawning duplicates.
    pub subagent_card_index: HashMap<String, usize>,
    /// History index of the most recent FanoutCard. Sibling sub-agents
    /// spawned by the same `rlm` invocation route into this card; reset
    /// when a fresh fanout-family tool call starts.
    pub last_fanout_card_index: Option<usize>,
    /// Most recently observed sub-agent dispatch tool name (set on
    /// `ToolCallStarted` for `agent` / `rlm` / etc., cleared
    /// after the first `Started` mailbox envelope routes through it).
    pub pending_subagent_dispatch: Option<String>,
    /// Animation anchor for status-strip active sub-agent spinner.
    pub agent_activity_started_at: Option<Instant>,
    /// Monotonic counter for stable agent labels (#3030).
    /// Incremented each time a sub-agent is spawned; used to generate
    /// "Agent 1", "Agent 2", etc.
    pub agent_counter: u64,
    /// Maps raw agent_id to a stable user-facing label (#3030).
    /// Populated when `AgentSpawned` fires; read by sidebar rendering.
    pub agent_label_map: HashMap<String, String>,
    /// Last time a sub-agent progress event triggered a redraw.
    /// Used to throttle redraws under high sub-agent concurrency (#3033).
    pub last_agent_progress_redraw: Option<Instant>,
    pub ui_theme: UiTheme,
    /// Active named theme. Drives the cell-level color remap in
    /// `tui::color_compat::ColorCompatBackend` so community presets
    /// (Catppuccin, Tokyo Night, Dracula, Gruvbox) propagate to every
    /// render site, not just the handful that read `app.ui_theme`.
    pub theme_id: palette::ThemeId,
    // Onboarding
    pub onboarding: OnboardingState,
    pub onboarding_needs_api_key: bool,
    pub onboarding_workspace_trust_gate: bool,
    pub api_key_env_only: bool,
    pub api_key_input: String,
    pub api_key_cursor: usize,
    // Hooks system
    pub hooks: HookExecutor,
    pub yolo: bool,
    /// Durable Agent-era permission baseline that Plan/YOLO derive from and
    /// restore to (#3386). Refreshed from the live fields whenever the user
    /// leaves Agent mode; see [`base_policy_for_mode`] and `set_mode`.
    pub(crate) mode_prefs: ModeSessionPrefs,
    // Clipboard handler
    pub clipboard: ClipboardHandler,
    // Tool approval session allowlist
    pub approval_session_approved: HashSet<String>,
    /// Approval keys (or tool names) the user has denied or aborted in
    /// this session. Subsequent re-requests for the same approval key
    /// auto-deny without re-prompting (#360) — the model can retry a
    /// dangerous command after being told no, but the user shouldn't
    /// have to keep dismissing the same dialog.
    pub approval_session_denied: HashSet<String>,
    pub approval_mode: ApprovalMode,
    // Modal view stack (approval/help/etc.)
    pub view_stack: ViewStack,
    /// Esc-Esc backtrack state machine (#133). `Inactive` by default; first
    /// Esc primes, second Esc opens the live-transcript overlay scoped to
    /// previous user messages so the user can rewind a turn.
    pub backtrack: crate::tui::backtrack::BacktrackState,
    /// Current session ID for auto-save updates
    pub current_session_id: Option<String>,
    /// Metadata-only registry of large tool outputs produced in this session.
    pub session_artifacts: Vec<ArtifactRecord>,
    /// Trust mode - allow access outside workspace
    pub trust_mode: bool,
    /// Translation mode — when enabled, the model is instructed to respond in
    /// the current locale and a post-hoc translation layer replaces any
    /// remaining English output before it reaches the user.
    pub translation_enabled: bool,
    /// Ordered list of footer items the user wants visible. Sourced from
    /// `tui.status_items` in `~/.mimofan/config.toml` at startup; mutated
    /// live by `/statusline`. The renderer iterates this slice; no item is
    /// hardcoded in the footer code path.
    pub status_items: Vec<crate::config::StatusItem>,
    /// Project documentation (AGENTS.md or CLAUDE.md)
    pub project_doc: Option<String>,
    /// Plan state for tracking tasks
    pub plan_state: SharedPlanState,
    /// Whether a plan follow-up prompt is waiting for user input
    pub plan_prompt_pending: bool,
    /// Whether update_plan was called during the current turn
    pub plan_tool_used_in_turn: bool,
    /// Todo list for `TodoWriteTool`. Read by the plan confirmation modal to
    /// show the active checklist alongside the plan.
    pub todos: SharedTodoList,
    /// Durable runtime services exposed to model-visible task/automation tools.
    pub runtime_services: RuntimeToolServices,
    /// Last MCP manager/discovery snapshot shown in the UI.
    pub mcp_snapshot: Option<crate::mcp::McpManagerSnapshot>,
    /// Number of MCP servers declared in the user's config at app boot.
    /// Used by the footer chip (#502) so a count is visible even before
    /// the user runs `/mcp` for the first time. `0` hides the chip.
    pub mcp_configured_count: usize,
    /// Set after in-TUI MCP config edits because the engine caches its MCP pool.
    pub mcp_restart_required: bool,
    /// Tool execution log
    pub tool_log: Vec<String>,
    /// Active skill to apply to next user message
    pub active_skill: Option<String>,
    /// Cached (name, description) pairs from the skill registry.
    /// Populated once at startup and refreshed on install/uninstall so
    /// the slash menu can show skills without filesystem I/O on every keystroke.
    pub cached_skills: Vec<(String, String)>,
    /// Tool call cells by tool id (for cells already finalized in `history`).
    /// While a tool call is in flight inside `active_cell`, it is tracked by
    /// `active_tool_entries` instead and migrated here at flush time.
    pub tool_cells: HashMap<String, usize>,
    /// Full tool input/output keyed by history cell index.
    pub tool_details_by_cell: HashMap<usize, ToolDetailRecord>,
    /// Linked context references keyed by the visible user history cell that
    /// introduced them.
    pub context_references_by_cell: HashMap<usize, Vec<SessionContextReference>>,
    /// Session-wide context references persisted with saved sessions.
    pub session_context_references: Vec<SessionContextReference>,
    /// In-flight tool/exec group for the current turn. Mutated in place as
    /// parallel tool calls start and complete; flushed into `history` on
    /// `TurnComplete`.
    pub active_cell: Option<ActiveCell>,
    /// Revision counter for `active_cell`. Combined with `active_cell.revision`
    /// when feeding the transcript cache so cached lines for the synthetic
    /// active-cell row are invalidated on every mutation.
    pub active_cell_revision: u64,
    /// Pending tool details for entries that live inside `active_cell`.
    /// Keyed by tool id rather than cell index because the active cell's
    /// virtual index can shift (orphan completions push real cells in
    /// between). Migrated into `tool_details_by_cell` on flush.
    pub active_tool_details: HashMap<String, ToolDetailRecord>,
    /// Completion timestamps for entries still living inside `active_cell`.
    /// The transcript keeps completed entries until turn flush, but the
    /// sidebar can use these timestamps to let settled live rows expire.
    pub active_tool_entry_completed_at: HashMap<usize, Instant>,
    /// Active exploring cell entry index (within `active_cell.entries`).
    /// `None` once the active cell flushes or no exploring entry exists.
    pub exploring_cell: Option<usize>,
    /// Mapping of exploring tool ids to `(entry index in active_cell, entry
    /// within ExploringCell)`. Used to update individual exploring entries
    /// when their tools complete.
    pub exploring_entries: HashMap<String, (usize, usize)>,
    /// Tool calls that should be ignored by the UI
    pub ignored_tool_calls: HashSet<String>,
    /// Last exec wait command shown (for duplicate suppression)
    pub last_exec_wait_command: Option<String>,
    /// Current streaming assistant cell
    pub streaming_message_index: Option<usize>,
    /// True after a local cancel key has been handled and before the engine's
    /// authoritative TurnComplete arrives. Stream events already queued for
    /// the cancelled turn are ignored so text does not keep appearing after
    /// Ctrl+C/Esc returns focus to the composer.
    pub suppress_stream_events_until_turn_complete: bool,
    /// Index into `active_cell.entries` of the thinking entry currently being
    /// streamed. `None` when no thinking block is in flight. P2.3 routes
    /// thinking into the active cell so it groups visually with tool calls
    /// until the next assistant prose chunk flushes the group into history.
    pub streaming_thinking_active_entry: Option<usize>,
    /// Instant of the last throttled active-cell revision bump for the
    /// in-flight thinking stream (#1620). Reasoning chunks arrive faster than
    /// the eye can read, and each bump invalidates the active cell's wrap
    /// cache, forcing a full re-wrap. We debounce intermediate bumps to a
    /// time window so high-frequency thinking deltas no longer trigger a
    /// re-render per character. `None` means "no bump since the last
    /// finalize" so the first chunk of a block always renders immediately.
    pub thinking_revision_last_bump_at: Option<Instant>,
    /// Newline-gated streaming collector state.
    pub streaming_state: StreamingState,
    /// Live approximate output tokens for the current assistant stream.
    pub streaming_output_token_estimate: u64,
    /// Accumulated reasoning text
    pub reasoning_buffer: String,
    /// Live reasoning header extracted from bold text
    pub reasoning_header: Option<String>,
    /// Last completed reasoning block
    pub last_reasoning: Option<String>,
    /// Tool calls captured for the pending assistant message
    pub pending_tool_uses: Vec<(String, String, Value)>,
    /// User messages queued while a turn is running
    pub queued_messages: VecDeque<QueuedMessage>,
    /// Draft queued message being edited
    pub queued_draft: Option<QueuedMessage>,
    /// Legacy pending-steer bucket retained for session compatibility. New
    /// in-flight input uses Enter for same-turn steering and Tab for queued
    /// follow-ups; Esc only cancels the active turn.
    pub pending_steers: VecDeque<QueuedMessage>,
    /// Engine-rejected steers (e.g. a tool was already running and couldn't be
    /// cancelled cleanly). Surfaced in the pending-input preview so the user
    /// knows the steer was deferred to end-of-turn. Today no engine path
    /// produces these; the field is scaffolding for a future signalling
    /// channel and the bucket renders with a rejected-steer label when
    /// populated.
    pub rejected_steers: VecDeque<String>,
    /// Legacy resend flag for pending steer recovery.
    pub submit_pending_steers_after_interrupt: bool,
    /// Start time for current turn
    pub turn_started_at: Option<Instant>,
    /// Wall-clock instant when the first content `MessageDelta` arrived.
    /// Used to compute TTFT (time-to-first-token) for the footer chip.
    pub turn_first_token_at: Option<Instant>,
    /// Most recent engine event observed for the current turn. This is
    /// separate from `turn_started_at` because the latter drives elapsed-time
    /// UI and must not be reset during long but healthy turns.
    pub turn_last_activity_at: Option<Instant>,
    /// Sum of completed turn durations for this `App` instance (#448
    /// follow-up). Drives the footer's `worked Nh Mm` chip so the
    /// label reflects actual model work, not wall-clock since launch.
    /// Incremented on `TurnComplete` from the elapsed time of the
    /// just-finished turn. Resets per launch.
    pub cumulative_turn_duration: std::time::Duration,
    /// DeepSeek account balance, refreshed once per turn completion.
    /// Shared cell updated by background fetch tasks; read lock in the UI thread.
    pub balance_cell: std::sync::Arc<std::sync::Mutex<Option<crate::pricing::BalanceInfo>>>,
    /// Shared cell for async prompt suggestion delivery from background task.
    pub prompt_suggestion_cell: std::sync::Arc<std::sync::Mutex<Option<(u64, String)>>>,
    /// Tracks whether the initial balance fetch has been attempted for this session.
    pub balance_initiated: bool,
    /// Timestamp of the last balance fetch, used to debounce rapid requests.
    pub last_balance_fetch: Option<std::time::Instant>,
    /// Current runtime turn id (if known).
    pub runtime_turn_id: Option<String>,
    /// Current runtime turn status (if known).
    pub runtime_turn_status: Option<String>,
    /// Monotonic turn counter for stable user-facing labels (#3030).
    /// Incremented each time a new turn starts; displayed as "Turn N".
    pub turn_counter: u64,
    /// When the UI accepted a user message but has not observed `TurnStarted` yet.
    pub dispatch_started_at: Option<Instant>,

    /// Cached git context snapshot for the footer.
    pub workspace_context: Option<String>,
    /// Shared cell for async git context updates (#399 S1).
    pub workspace_context_cell: std::sync::Arc<std::sync::Mutex<Option<String>>>,
    /// Timestamp for cached workspace context.
    pub workspace_context_refreshed_at: Option<Instant>,
    /// Cached background tasks for sidebar rendering.
    pub task_panel: Vec<TaskPanelEntry>,
    /// Active decision card (v0.8.43 truth-surface). When set, keyboard input
    /// is routed through the card navigation instead of the composer.
    pub decision_card: Option<crate::tui::widgets::decision_card::DecisionCard>,
    /// Wall-clock time when this TUI session started. Used by the Work
    /// sidebar projection to hide completed durable tasks that finished
    /// before the current session (bug #1913).
    pub session_started_at: chrono::DateTime<chrono::Utc>,
    /// Whether the UI needs to be redrawn.
    pub needs_redraw: bool,
    /// When true, the next draw will be a full repaint (terminal clear +
    /// all cells redrawn) instead of a ratatui incremental diff. Used by
    /// theme switches where the diff engine may miss color-only changes
    /// in sidebar cells that were previously rendered with palette constants.
    pub force_next_full_repaint: bool,
    /// When the current thinking block started (for duration tracking).
    pub thinking_started_at: Option<Instant>,
    /// Whether context compaction is currently in progress.
    pub is_compacting: bool,
    /// Whether context purge is currently in progress.
    pub is_purging: bool,
    /// Set when the user scrolls up/down during a streaming turn so subsequent
    /// streamed chunks don't yank the view back to the live tail. Cleared
    /// when the user explicitly returns to bottom or the turn completes.
    pub user_scrolled_during_stream: bool,
    /// Timestamp of the last user message send (for brief visual feedback).
    pub last_send_at: Option<Instant>,
    /// Most recent user prompt accepted for an active engine turn. Ctrl+C can
    /// restore this into an empty composer after cancelling that turn.
    pub last_submitted_prompt: Option<String>,
    /// Startup prompt should be submitted automatically after the engine is ready.
    pub auto_submit_initial_input: bool,
    /// Two-tap quit confirmation. When set, a prior Ctrl+C in idle state has
    /// armed the quit shortcut; a second Ctrl+C before this `Instant` exits
    /// the app, while expiry silently re-arms the prompt for next time.
    /// Stays `None` while a turn is in flight or a modal/picker is open so
    /// Ctrl+C keeps its current "interrupt this turn" semantics in those
    /// states. See [`App::arm_quit`] / [`App::quit_is_armed`].
    pub quit_armed_until: Option<Instant>,

    // === Prefix-Cache Stability Tracking ===
    /// Number of times the prefix (system prompt + tool specs) has changed.
    pub prefix_change_count: u64,
    /// Total number of prefix stability checks performed.
    pub prefix_checks_total: u64,
    /// Current prefix stability percentage, if known.
    pub prefix_stability_pct: Option<u32>,
    /// Description of the last prefix change, if any.
    pub last_prefix_change_desc: Option<String>,
    /// Current pinned prefix combined hash (SHA-256, 64 hex chars).
    /// Updated per-turn via PrefixCacheChange events; surfaced by
    /// `/cache stats` for cache-hit debugging.
    pub last_pinned_prefix_hash: Option<String>,

    // === Transcript filtering (#397) ===
    /// Transcript cells the user has collapsed (hidden from view).
    /// Stores **original** virtual cell indices (pre-filtering).
    pub collapsed_cells: HashSet<usize>,
    /// Thinking cells the user has folded (showing summary instead of full
    /// content). Stores **original** virtual cell indices. Toggled by Space
    /// when the composer is empty and the cursor is on a thinking cell.
    pub folded_thinking: HashSet<usize>,
    /// Mapping from filtered cell index → original virtual index.
    /// Populated during `ChatWidget::new` by filtering out collapsed cells.
    /// Used by `build_context_menu_entries` to convert line-meta indices
    /// back to original indices for the `HideCell` / `ShowCell` actions.
    pub collapsed_cell_map: Vec<usize>,

    /// Whether `/edit` has loaded the last user message into the composer and
    /// the next submit should replace (not append to) the last exchange.
    pub edit_in_progress: bool,

    /// Whether LSP diagnostics are currently enabled. Mirrors the config file
    /// `[lsp].enabled` setting. Toggled at runtime via `/lsp on|off`.
    pub lsp_enabled: bool,
    /// Derived title for the current session shown in the composer border.
    /// Updated when `EngineEvent::SessionUpdated` fires or a saved session is loaded.
    pub session_title: Option<String>,

    /// Post-turn receipt rendered as transient composer chrome.
    /// Set when a turn completes; cleared when a new turn starts or after expiry.
    pub receipt_text: Option<String>,
    pub receipt_started_at: Option<Instant>,
    /// Tool evidence collected during the current turn for the receipt.
    pub tool_evidence: Vec<ToolEvidence>,

    // === Spec Freeze (#557) ===
    /// Whether the current spec/plan is frozen. When true, the agent must not
    /// deviate from the frozen spec without explicit user approval.
    pub spec_frozen: bool,
    /// The frozen spec content. Populated when `/freeze` is called; cleared on
    /// `/unfreeze`. Injected into the system prompt as a hard constraint.
    pub frozen_spec: Option<String>,
}

// === Deref to ComposerState for backward compat ===

impl std::ops::Deref for App {
    type Target = ComposerState;
    fn deref(&self) -> &Self::Target {
        &self.composer
    }
}

impl std::ops::DerefMut for App {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.composer
    }
}

// === Errors ===

/// Errors that can occur while submitting API keys during onboarding.
#[derive(Debug, thiserror::Error)]
pub enum ApiKeyError {
    /// The provided API key was empty.
    #[error("Failed to save API key: API key cannot be empty")]
    Empty,
    /// Persisting the API key failed.
    #[error("Failed to save API key: {source}")]
    SaveFailed { source: anyhow::Error },
}
