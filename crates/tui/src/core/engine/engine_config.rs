//! Engine configuration types.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use mimofan_config::catalog::ProviderCatalogCache;

use crate::compaction::CompactionConfig;
use crate::config::{DEFAULT_MAX_SUBAGENTS, DEFAULT_TEXT_MODEL};
use crate::features::Features;
use crate::tools::goal::{GoalStatus, SharedGoalQueue, new_shared_goal_queue};
use crate::tools::plan::{SharedPlanState, new_shared_plan_state};
use crate::tools::spec::{RuntimeToolServices, ToolSpec};
use crate::tools::todo::SharedTodoList;
use crate::tools::todo::new_shared_todo_list;

/// Configuration for the engine
#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// Model identifier to use for responses.
    pub model: String,
    /// Route/offering limits for the active provider+model, when the runtime
    /// route resolver had concrete catalog facts.
    pub active_route_limits: Option<mimofan_config::route::RouteLimits>,
    /// Workspace root for tool execution and file operations.
    pub workspace: PathBuf,
    /// Allow shell tool execution when true.
    pub allow_shell: bool,
    /// Enable trust mode (skip approvals) when true.
    pub trust_mode: bool,
    /// Path to the notes file used by the notes tool.
    pub notes_path: PathBuf,
    /// Path to the MCP configuration file.
    pub mcp_config_path: PathBuf,
    /// Directory containing discoverable skills.
    pub skills_dir: PathBuf,
    /// Restrict skill discovery to mimofan-owned roots plus explicit
    /// `skills_dir` configuration.
    pub skills_scan_mimofan_only: bool,
    /// Sources injected as `<instructions source="...">` blocks in the system
    /// prompt (#454). Each entry is either a disk path (read at render time)
    /// or an inline string. Loaded in declared order from the user's
    /// `instructions = [...]` config or constructed by embedders.
    ///
    /// Generalized from `Vec<PathBuf>` so embedders can inject inline content
    /// without staging a disk file. `From<PathBuf>` impl keeps existing callers
    /// working with `.into()` at the call site.
    pub instructions: Vec<crate::prompts::InstructionSource>,
    pub project_context_pack_enabled: bool,
    /// When true, the model is instructed to respond in the current locale
    /// and a post-hoc translation layer replaces remaining English output.
    pub translation_enabled: bool,
    /// Whether user-visible transcript rendering shows thinking blocks.
    /// Prompt assembly uses this to avoid localizing hidden reasoning.
    pub show_thinking: bool,
    pub verbosity: Option<String>,
    /// Maximum number of assistant steps before stopping.
    pub max_steps: u32,
    /// Maximum number of concurrently active subagents.
    pub max_subagents: usize,
    /// Maximum queued + running sub-agents admitted for this engine session.
    pub max_admitted_subagents: usize,
    /// Number of direct (depth-1) sub-agents that may execute concurrently
    /// before further launches queue for a launch slot (#3095).
    /// Resolved from `[subagents] launch_concurrency`.
    pub launch_concurrency: usize,
    /// Whether the model-facing `agent` tool is available after applying
    /// feature flags and `[subagents]` opt-out controls.
    pub subagents_enabled: bool,
    /// Feature flags controlling tool availability.
    pub features: Features,
    /// Deterministic auto-review policy for tool calls.
    pub auto_review_policy: crate::tui::auto_review::AutoReviewPolicy,
    /// Auto-compaction settings for long conversations.
    pub compaction: CompactionConfig,
    /// Shared Todo list state.
    pub todos: SharedTodoList,
    /// Shared Plan state.
    pub plan_state: SharedPlanState,
    /// Shared goal queue (multi-objective scheduling) for model-visible goal tools.
    pub goal_queue: SharedGoalQueue,
    /// Maximum sub-agent recursion depth (default 3). See
    /// `SubAgentRuntime::max_spawn_depth`. Override via
    /// `[subagents] max_depth = N` in `~/.mimofan/config.toml`.
    pub max_spawn_depth: u32,
    /// Optional aggregate token budget for each root sub-agent run.
    /// Descendant agents inherit the root pool unless a child starts a new
    /// budget scope with an explicit per-call override.
    pub subagent_token_budget: Option<u64>,
    /// Per-domain network policy decider (#135). Shared across the session so
    /// session-scoped approvals (`/network allow <host>`) persist for the
    /// remainder of the run.
    pub network_policy: Option<crate::network_policy::NetworkPolicyDecider>,
    /// Whether to take side-git workspace snapshots before/after each turn.
    pub snapshots_enabled: bool,
    /// Maximum workspace size (in bytes) before snapshots self-disable on
    /// first init. `0` disables the cap. Resolved from
    /// `[snapshots] max_workspace_gb` x 1 GB at engine construction.
    pub snapshots_max_workspace_bytes: u64,
    /// Post-edit LSP diagnostics injection (#136). When `None`, the engine
    /// constructs a disabled manager so the field is always present.
    pub lsp_config: Option<crate::lsp::LspConfig>,
    /// Durable runtime services exposed to model-visible tools.
    pub runtime_services: RuntimeToolServices,
    /// Per-role/type sub-agent model overrides already resolved from config.
    pub subagent_model_overrides: HashMap<String, String>,
    /// Whether the user-memory feature is enabled (#489). When `true` the
    /// engine reads the memory index from `memory_dir` on each prompt
    /// assembly and prepends a `<user_memory_index>` block to the system
    /// prompt.
    pub memory_enabled: bool,
    /// Path to the user memory directory (#489). Always populated; only
    /// consulted when `memory_enabled` is `true`.
    pub memory_dir: PathBuf,
    /// Default directory for Xiaomi MiMo speech/TTS tool outputs.
    pub speech_output_dir: Option<PathBuf>,
    pub vision_config: Option<crate::config::VisionModelConfig>,
    pub goal_objective: Option<String>,
    pub goal_token_budget: Option<u32>,
    pub goal_status: GoalStatus,
    /// Tool restriction from custom slash command frontmatter.
    /// `None` means the current turn may use the normal tool set.
    pub allowed_tools: Option<Vec<String>>,
    /// Tool deny-list.  Deny always wins over allow (#3027).
    /// `None` means no tools are explicitly denied.
    pub disallowed_tools: Option<Vec<String>>,
    /// Hook executor for control-plane hooks.
    /// `ToolCallBefore` hooks may deny a tool call with exit code 2.
    pub hook_executor: Option<std::sync::Arc<crate::hooks::HookExecutor>>,
    /// Resolved BCP-47 locale tag (e.g. `"en"`, `"zh-Hans"`, `"ja"`)
    /// for the `## Environment` block in the system prompt. The
    /// caller resolves this from `Settings` once at engine
    /// construction; the engine never touches disk for it.
    pub locale_tag: String,
    /// When true, force `tool_choice: "required"` and opt compatible function
    /// schemas into DeepSeek beta strict mode.
    pub strict_tool_mode: bool,
    /// Workshop / large-tool-output routing (#548). `None` disables routing.
    pub workshop: Option<crate::tools::large_output_router::WorkshopConfig>,
    /// Which search backend `web_search` should use. Default: DuckDuckGo.
    pub search_provider: crate::config::SearchProvider,
    /// API key for Tavily, Bocha, Metaso, or Baidu. `None` for Bing or DuckDuckGo.
    /// Metaso also falls back to `METASO_API_KEY` env var, then a built-in key.
    /// Baidu also falls back to `BAIDU_SEARCH_API_KEY`.
    pub search_api_key: Option<String>,
    /// Optional DuckDuckGo-compatible HTML endpoint override.
    pub search_base_url: Option<String>,
    /// Per-step DeepSeek API timeout for sub-agent `create_message` requests.
    /// Resolved from `[subagents] api_timeout_secs` (clamped to 1..=1800)
    /// once at engine construction, then threaded onto every
    /// `SubAgentRuntime` the engine builds (#1806, #1808).
    pub subagent_api_timeout: Duration,
    /// Per-SSE-chunk idle timeout for streamed model responses.
    /// Resolved from `[tui].stream_chunk_timeout_secs` (or the legacy
    /// `MIMOFAN_STREAM_IDLE_TIMEOUT_SECS`) and updated live by `/config`.
    pub stream_chunk_timeout: Duration,
    /// No-progress heartbeat timeout for live sub-agents. Used by the manager
    /// and parent wait loop to auto-cancel stuck children before they exhaust
    /// the sub-agent slot pool indefinitely (#2614).
    pub subagent_heartbeat_timeout: Duration,
    /// Native tools that should stay in the model-visible catalog even when
    /// they are outside the small default core surface (#2076).
    pub tools_always_load: HashSet<String>,
    /// When true and `/usr/bin/bwrap` is present on Linux, route exec_shell
    /// through bubblewrap instead of relying solely on Landlock (#2184).
    pub prefer_bwrap: bool,
    /// Tool override and plugin configuration (`[tools]` table in config.toml).
    /// Applied to the per-turn tool registry after built-in tools are registered.
    /// When `None`, no overrides or plugin loading occurs.
    pub tools: Option<crate::config::ToolsConfig>,
    /// Frozen spec content for spec freeze (#557). When set, injected into
    /// the system prompt as a hard constraint that the agent must not deviate from.
    pub frozen_spec: Option<String>,
    /// Whether tools should follow symbolic links. When `true`, symlinked
    /// directories are traversed by walk-based tools and symlinked paths
    /// that resolve outside the workspace are still allowed (the symlink
    /// itself must be inside the workspace). Mirrors the
    /// `workspace_follow_symlinks` setting.
    pub workspace_follow_symlinks: bool,
    /// Ask-only permission rules loaded from sibling `permissions.toml`.
    pub exec_policy_engine: mimofan_execpolicy::ExecPolicyEngine,
    /// Shared, process-wide live catalog cache (global, multi-provider). The
    /// same `Arc` is shared with `App` so a refresh from the engine is visible
    /// to the provider picker and vice versa (#3385).
    pub catalog_cache: Arc<StdMutex<ProviderCatalogCache>>,
    /// Caller-injected tools registered into every turn's tool registry
    /// (in addition to the built-in set). Used by headless `--json-schema`
    /// runs to mount the synthetic terminator tool (#824). Empty by default,
    /// so legacy behavior is unchanged.
    pub extra_tools: ExtraTools,
    /// When true, non-interactive LLM requests are collected and submitted via
    /// the offline Batch API channel instead of synchronous calls, cutting
    /// cost (~50%). Added for #844 — purely additive: defaults to `false`, so
    /// existing field order, types, and construction sites are unchanged.
    pub batch_mode: bool,
    /// When `Some`, an aggregate token budget for the whole task/goal the
    /// engine decrements per turn and halts when exhausted (#848). Added for
    /// #848 — purely additive: defaults to `None` (unbounded), so existing
    /// field order, types, and construction sites are unchanged.
    pub task_budget_tokens: Option<usize>,
    /// Optional path to a prior session dir or state file; when set the engine
    /// auto-resumes from the last completed turn boundary on start (#857).
    /// Added for #857 — purely additive, defaults to `None`.
    pub resume_session: Option<PathBuf>,
    /// Optional validate-then-retry policy: when a turn fails an objective
    /// validation check, the engine escalates effort/model and retries,
    /// bounded by `policy.max_escalations` (#845). Added for #845 — purely
    /// additive, defaults to `None` (no automatic retry).
    pub validation_retry: Option<crate::core::engine::resilience::ValidationRetryConfig>,
    /// Run the engine in fully headless / unattended mode (#853). When `true`
    /// the tool registry is restricted to a SAFE subset (read-only + auto
    /// approval; no human-approval or destructive tools) so the run never
    /// blocks on input. Combined with #863's `HeadlessGate` (budget / max-turn
    /// cap / failure log) to make the run terminate safely on any error.
    /// Added for #853 — purely additive, defaults to `false`.
    pub unattended: bool,
    /// Period (in completed turns) at which the memory `ConsolidationScheduler`
    /// triggers a consolidation pass (#855). `None` disables periodic
    /// consolidation; otherwise overrides the scheduler's built-in default
    /// interval. Added for #855 — purely additive, defaults to `None`
    /// (scheduler default applies).
    pub consolidation_interval_turns: Option<usize>,
    /// Path to which the headless gate writes a structured failure event when
    /// unattended mode ends on an unrecoverable error (#863). `None` falls back
    /// to a default `<workspace>/.mimofan/failures.jsonl`. Validated by
    /// `HeadlessGate` at startup when `unattended` is set.
    /// Added for #863 — purely additive, defaults to `None`.
    pub failure_log_path: Option<std::path::PathBuf>,
    /// When `true`, after every successful context compaction the engine asks
    /// the model to re-confirm its original objective via a bounded, advisory
    /// self-check nudge. The nudge is injected over the *system* prompt channel
    /// (never as a user message) so it cannot pollute the conversation history,
    /// guarding long-horizon tasks against silent goal drift. Opt-out only —
    /// defaults to `false` so existing behaviour is unchanged until enabled.
    pub goal_self_check_after_compact: bool,
    /// Whether/how to record a per-session trajectory JSONL by default
    /// (redacted) to `~/.mimofan/tasks/<session_id>/session.jsonl`. Opt-out
    /// via `[session_trace] enabled = false`.
    pub session_trace: crate::config::SessionTraceConfig,
}

/// Wrapper around a list of injected `ToolSpec` implementations so it can live
/// on the `Clone + Debug` `EngineConfig` without requiring every `ToolSpec`
/// impl to also be `Debug`. `Debug` renders only the count (the tool set is
/// owned by the caller, not the engine).
#[derive(Clone, Default)]
pub struct ExtraTools(pub Vec<Arc<dyn ToolSpec>>);

impl std::fmt::Debug for ExtraTools {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExtraTools")
            .field("count", &self.0.len())
            .finish()
    }
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            model: DEFAULT_TEXT_MODEL.to_string(),
            active_route_limits: None,
            workspace: PathBuf::from("."),
            allow_shell: true,
            trust_mode: false,
            notes_path: PathBuf::from("notes.txt"),
            mcp_config_path: PathBuf::from("mcp.json"),
            skills_dir: crate::skills::default_skills_dir(),
            skills_scan_mimofan_only: false,
            instructions: Vec::new(),
            project_context_pack_enabled: true,
            translation_enabled: false,
            show_thinking: true,
            // High backstop rather than a working ceiling. Repetition is
            // braked in-turn by `crate::loop_guard`, which injects a bounded
            // self-correction hint on repeat / A-B-A-B / no-progress patterns;
            // this only exists to terminate a pathological runaway turn via
            // `at_max_steps()`. 1000 stays high enough to never gate real work
            // while still guaranteeing the turn ends.
            max_steps: 1000,
            max_subagents: DEFAULT_MAX_SUBAGENTS,
            max_admitted_subagents: DEFAULT_MAX_SUBAGENTS,
            launch_concurrency: DEFAULT_MAX_SUBAGENTS,
            subagents_enabled: true,
            features: Features::with_defaults(),
            auto_review_policy: crate::tui::auto_review::AutoReviewPolicy::default(),
            compaction: CompactionConfig::default(),
            todos: new_shared_todo_list(),
            plan_state: new_shared_plan_state(),
            goal_queue: new_shared_goal_queue(),
            max_spawn_depth: crate::tools::subagent::DEFAULT_MAX_SPAWN_DEPTH,
            subagent_token_budget: None,
            network_policy: None,
            snapshots_enabled: true,
            snapshots_max_workspace_bytes:
                crate::snapshot::DEFAULT_MAX_WORKSPACE_BYTES_FOR_SNAPSHOT,
            lsp_config: None,
            runtime_services: RuntimeToolServices::default(),
            subagent_model_overrides: HashMap::new(),
            memory_enabled: false,
            memory_dir: PathBuf::from("./memory"),
            speech_output_dir: None,
            vision_config: None,
            strict_tool_mode: false,
            goal_objective: None,
            goal_token_budget: None,
            goal_status: GoalStatus::Active,
            allowed_tools: None,
            disallowed_tools: None,
            hook_executor: None,
            locale_tag: "en".to_string(),
            workshop: None,
            search_provider: crate::config::SearchProvider::default(),
            search_api_key: None,
            search_base_url: None,
            subagent_api_timeout: Duration::from_secs(
                crate::config::DEFAULT_SUBAGENT_API_TIMEOUT_SECS,
            ),
            stream_chunk_timeout: Duration::from_secs(
                crate::config::DEFAULT_STREAM_CHUNK_TIMEOUT_SECS,
            ),
            subagent_heartbeat_timeout: Duration::from_secs(
                crate::config::DEFAULT_SUBAGENT_HEARTBEAT_TIMEOUT_SECS,
            ),
            tools_always_load: HashSet::new(),
            prefer_bwrap: false,
            verbosity: None,
            tools: None,
            workspace_follow_symlinks: false,
            exec_policy_engine: mimofan_execpolicy::ExecPolicyEngine::new(Vec::new(), Vec::new()),
            frozen_spec: None,
            catalog_cache: Arc::new(StdMutex::new(ProviderCatalogCache::new())),
            extra_tools: ExtraTools::default(),
            batch_mode: false,
            task_budget_tokens: None,
            resume_session: None,
            validation_retry: None,
            unattended: false,
            consolidation_interval_turns: None,
            failure_log_path: None,
            goal_self_check_after_compact: false,
            session_trace: crate::config::SessionTraceConfig::default(),
        }
    }
}
