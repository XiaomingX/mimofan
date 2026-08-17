//! Core engine for `DeepSeek` CLI.
//!
//! The engine handles all AI interactions in a background task,
//! communicating with the UI via channels. This enables:
//! - Non-blocking UI during API calls
//! - Real-time streaming updates
//! - Proper cancellation support
//! - Tool execution orchestration

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant, SystemTime};

use anyhow::Result;
use futures_util::StreamExt;
use futures_util::stream::FuturesUnordered;
use mimofan_protocol::runtime::DynamicToolSpec;
use serde_json::json;
use tokio::sync::{Mutex as AsyncMutex, RwLock, mpsc};
use tokio_util::sync::CancellationToken;

use crate::client::ApiClient;
use crate::compaction::{
    compact_messages_safe, compact_messages_safe_with_objective, merge_system_prompts,
    should_compact,
};
use crate::config::{ApiProvider, Config};
use crate::context_budget::ContextBudget;
use crate::error_taxonomy::{ErrorCategory, ErrorEnvelope, StreamError};
use crate::features::Feature;
use crate::llm_client::LlmClient;
use crate::mcp::{McpPool, PendingMcpCall, PreparedMcpCall};

use crate::models::{
    ContentBlock, ContentBlockStart, Delta, Message, MessageRequest, StreamEvent, SystemBlock,
    SystemPrompt, Tool, Usage,
};
use crate::prompts;
use crate::purge::{emit_purge_completed, emit_purge_failed, emit_purge_started, run_purge};
use crate::route_runtime::resolve_runtime_route;
use crate::seam_manager::{SeamConfig, SeamManager};
use crate::tools::goal::{GoalSnapshot, GoalStatus};
use crate::tools::plan::{PlanSnapshot, SharedPlanState};
use crate::tools::shell::{SharedShellManager, new_shared_shell_manager};
use crate::tools::spec::{ApprovalRequirement, ToolError, ToolResult};
use crate::tools::subagent::{
    Mailbox, MailboxMessage, SharedSubAgentManager, SubAgentCompletion, SubAgentForkContext,
    SubAgentResult, SubAgentRuntime, SubAgentStatus, SubAgentThinking, SubAgentType,
    new_shared_subagent_manager_with_timeout, resolve_subagent_assignment_route,
};
use crate::tools::todo::{SharedTodoList, TodoListSnapshot};
use crate::tools::user_input::{UserInputRequest, UserInputResponse};
use crate::tools::{ToolContext, ToolRegistryBuilder};
use crate::tui::app::AppMode;
use crate::utils::spawn_supervised;
use crate::worker_profile::ModelRoute;
use crate::working_set::WorkingSet;

use super::events::{Event, TurnOutcomeStatus};
use super::ops::{Op, SessionSnapshot, USER_SHELL_TOOL_ID_PREFIX, UserInputProvenance};
use super::session::Session;
use super::tool_parser;
use super::turn::{TurnContext, post_turn_snapshot, pre_turn_snapshot};

/// Snapshot of parent state that can be passed to forked sub-agents without
/// rewriting the parent transcript.
#[derive(Debug, Clone, Default)]
struct StructuredState {
    mode_label: String,
    workspace: PathBuf,
    cwd: Option<PathBuf>,
    working_set_summary: Option<String>,
    todo_snapshot: Option<TodoListSnapshot>,
    plan_snapshot: Option<PlanSnapshot>,
    subagent_snapshots: Vec<SubAgentResult>,
}

impl StructuredState {
    async fn capture(
        mode_label: impl Into<String>,
        workspace: PathBuf,
        cwd: Option<PathBuf>,
        working_set: &WorkingSet,
        todos: &SharedTodoList,
        plan_state: &SharedPlanState,
        subagents: Option<&SharedSubAgentManager>,
    ) -> Self {
        let working_set_summary = working_set.summary_block(&workspace);

        let todo_snapshot = {
            let guard = todos.lock().await;
            let snap = guard.snapshot();
            if snap.items.is_empty() {
                None
            } else {
                Some(snap)
            }
        };

        let plan_snapshot = {
            let guard = plan_state.lock().await;
            if guard.is_empty() {
                None
            } else {
                Some(guard.snapshot())
            }
        };

        let subagent_snapshots = if let Some(handle) = subagents {
            let guard = handle.read().await;
            guard
                .list()
                .into_iter()
                .filter(|s| matches!(s.status, SubAgentStatus::Running))
                .collect()
        } else {
            Vec::new()
        };

        Self {
            mode_label: mode_label.into(),
            workspace,
            cwd,
            working_set_summary,
            todo_snapshot,
            plan_snapshot,
            subagent_snapshots,
        }
    }

    #[must_use]
    fn to_system_block(&self) -> Option<String> {
        let mut out = String::new();
        out.push_str("## Fork State\n\n");
        out.push_str(&format!("- Mode: `{}`\n", self.mode_label));
        out.push_str(&format!("- Workspace: `{}`\n", self.workspace.display()));
        if let Some(cwd) = self.cwd.as_ref() {
            out.push_str(&format!("- Cwd: `{}`\n", cwd.display()));
        }

        if self.todo_snapshot.is_some() || self.plan_snapshot.is_some() {
            out.push_str("\n### Work\n");
        }

        if let Some(todos) = self.todo_snapshot.as_ref() {
            out.push_str(&format!(
                "\nChecklist ({}% complete)\n",
                todos.completion_pct
            ));
            for item in &todos.items {
                let marker = match item.status {
                    crate::tools::todo::TodoStatus::Pending => "[ ]",
                    crate::tools::todo::TodoStatus::InProgress => "[~]",
                    crate::tools::todo::TodoStatus::Completed => "[x]",
                };
                out.push_str(&format!("- {marker} {}\n", item.content));
            }
        }

        if let Some(plan) = self.plan_snapshot.as_ref() {
            out.push_str("\nStrategy metadata\n");
            append_plan_field(&mut out, "Title", plan.title.as_deref());
            append_plan_field(&mut out, "Objective", plan.objective.as_deref());
            append_plan_field(&mut out, "Context", plan.context_summary.as_deref());
            append_plan_field(&mut out, "Explanation", plan.explanation.as_deref());
            append_plan_list(&mut out, "Source", &plan.sources_used);
            append_plan_list(&mut out, "Critical file", &plan.critical_files);
            append_plan_list(&mut out, "Constraint", &plan.constraints);
            append_plan_field(
                &mut out,
                "Recommended approach",
                plan.recommended_approach.as_deref(),
            );
            append_plan_field(
                &mut out,
                "Verification plan",
                plan.verification_plan.as_deref(),
            );
            append_plan_field(
                &mut out,
                "Risks and unknowns",
                plan.risks_and_unknowns.as_deref(),
            );
            append_plan_field(&mut out, "Handoff packet", plan.handoff_packet.as_deref());
            for item in &plan.items {
                let marker = match item.status {
                    crate::tools::plan::StepStatus::Pending => "[ ]",
                    crate::tools::plan::StepStatus::InProgress => "[~]",
                    crate::tools::plan::StepStatus::Completed => "[x]",
                };
                out.push_str(&format!("- {marker} {}\n", item.step));
            }
        }

        if !self.subagent_snapshots.is_empty() {
            out.push_str("\n### Open Sub-Agents\n");
            for s in &self.subagent_snapshots {
                let role = s.assignment.role.as_deref().unwrap_or("-");
                let goal = if s.assignment.objective.is_empty() {
                    "(no objective set)"
                } else {
                    s.assignment.objective.as_str()
                };
                out.push_str(&format!("- `{}` (role: {}) - {}\n", s.agent_id, role, goal));
            }
        }

        if let Some(working_set) = self.working_set_summary.as_deref() {
            out.push('\n');
            out.push_str(working_set);
            out.push('\n');
        }

        Some(out)
    }
}

fn append_plan_field(out: &mut String, label: &str, value: Option<&str>) {
    if let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) {
        out.push_str(&format!("- {label}: {value}\n"));
    }
}

fn append_plan_list(out: &mut String, label: &str, values: &[String]) {
    for value in values {
        let value = value.trim();
        if !value.is_empty() {
            out.push_str(&format!("- {label}: {value}\n"));
        }
    }
}

// === Types ===

/// Reason the active turn was cancelled. The token from `tokio_util`
/// does not carry a cause, so the engine keeps a sibling latch for
/// approval and user-input waits that need to explain cancellation.
///
/// `External`, `Preempted`, and `Internal` are reserved for the
/// remaining direct cancellation paths tracked in #1541.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelReason {
    /// User-initiated cancel (Esc, `/cancel`, click cancel on modal).
    User,
    /// External / runtime-API cancel (HTTP `DELETE /v1/threads/...`,
    /// task manager stop, parent agent cancel).
    External,
    /// Cancel triggered when a new turn starts before the previous one
    /// finished — e.g. plain Enter while busy after the queueing path
    /// pre-empts the running turn.
    Preempted,
    /// Engine internals tore down the turn (drop, channel close,
    /// shutdown). Rare — surfaced as an internal error.
    Internal,
}

impl CancelReason {
    fn describe(self) -> &'static str {
        match self {
            Self::User => "user cancelled the request",
            Self::External => "request cancelled by external caller",
            Self::Preempted => "request was preempted by a new turn",
            Self::Internal => "engine torn down before approval resolved",
        }
    }
}

/// Handle to communicate with the engine
#[derive(Clone)]
pub struct EngineHandle {
    /// Send operations to the engine
    pub tx_op: mpsc::Sender<Op>,
    /// Receive events from the engine
    pub rx_event: Arc<RwLock<mpsc::Receiver<Event>>>,
    /// Shared pointer to the cancellation token for the current request.
    cancel_token: Arc<StdMutex<CancellationToken>>,
    /// Latched reason for the most recent cancellation. Read by the
    /// approval / user-input handlers to enrich their error strings.
    /// Cleared by the engine when a fresh turn starts.
    cancel_reason: Arc<StdMutex<Option<CancelReason>>>,
    /// Send approval decisions to the engine
    tx_approval: mpsc::Sender<ApprovalDecision>,
    /// Send user input responses to the engine
    tx_user_input: mpsc::Sender<UserInputDecision>,
    /// Send steer input for an in-flight turn.
    tx_steer: mpsc::Sender<String>,
    /// Shared pause flag set by the TUI and read by the turn loop.
    shared_paused: Arc<StdMutex<bool>>,
}

// `impl EngineHandle { ... }` moved to `engine/handle.rs` so the
// mailbox API can be reviewed independently of the engine internals.

// === Engine ===

/// The core engine that processes operations and emits events
pub struct Engine {
    config: EngineConfig,
    api_config: Config,
    deepseek_client: Option<ApiClient>,
    deepseek_client_error: Option<String>,
    api_key_env_only_recovery: Option<String>,
    session: Session,
    subagent_manager: SharedSubAgentManager,
    shell_manager: SharedShellManager,
    mcp_pool: Option<Arc<AsyncMutex<McpPool>>>,
    api_provider: ApiProvider,
    active_route_limits: Option<mimofan_config::route::RouteLimits>,
    rx_op: mpsc::Receiver<Op>,
    /// Clone of the op-channel sender, so the engine can self-dispatch ops
    /// (e.g. a goal-continuation `SendMessage` after a turn completes).
    tx_op: mpsc::Sender<Op>,
    rx_approval: mpsc::Receiver<ApprovalDecision>,
    rx_user_input: mpsc::Receiver<UserInputDecision>,
    rx_steer: mpsc::Receiver<String>,
    tx_event: mpsc::Sender<Event>,
    /// Wakeup channel for the parent turn loop when a direct child sub-agent
    /// terminates (issue #756). Cloned into `SubAgentRuntime` so the runtime
    /// can fan completion events back into the engine.
    tx_subagent_completion: mpsc::UnboundedSender<SubAgentCompletion>,
    /// Receiver paired with `tx_subagent_completion`. Drained at the
    /// turn-loop's empty-tool_uses branch to surface `<mimo:subagent.done>`
    /// sentinels into the parent's transcript before deciding to end the turn.
    pub(super) rx_subagent_completion: mpsc::UnboundedReceiver<SubAgentCompletion>,
    /// Sub-agent completions already injected into the parent transcript.
    /// Channel delivery and watchdog reconciliation both mark this set so a
    /// dropped event can be synthesized once without duplicating a later
    /// delivery.
    delivered_subagent_completion_ids: HashSet<String>,
    cancel_token: CancellationToken,
    shared_cancel_token: Arc<StdMutex<CancellationToken>>,
    /// Latched reason for the current cancellation, mirrored to
    /// `EngineHandle::cancel_reason`. Read by `approval.rs` when
    /// surfacing the "Request cancelled while awaiting …" error so the
    /// user-facing message names a cause.
    pub(super) cancel_reason: Arc<StdMutex<Option<CancelReason>>>,
    tool_exec_lock: Arc<RwLock<()>>,
    /// Append-only layered context manager (#159). Opt-in for v0.7.5 while
    /// cache-hit behavior is audited.
    seam_manager: Option<SeamManager>,
    turn_counter: u64,
    /// Latched once the vector-memory injection has been attempted on the
    /// first turn, so we only do the (network) recall once per session.
    #[cfg(feature = "vector-memory")]
    vector_memory_injected: bool,
    /// Recalled `<vector_memory>` block from the first-turn semantic recall.
    /// Stored so `refresh_system_prompt` can re-append it after a context
    /// refresh would otherwise drop the injected block.
    #[cfg(feature = "vector-memory")]
    vector_memory_block: Option<String>,
    /// Cross-session `UserProfile` (#732): loaded once at engine init and
    /// injected into the system prompt so the model "remembers" stable user
    /// preferences/constraints across sessions. Distilled and saved back at
    /// session end via `auto_capture_memory`.
    user_profile: Option<crate::memory::UserProfile>,
    /// Post-edit LSP diagnostics injection (#136). Populated unconditionally
    /// — when LSP is disabled in config, this is an inert manager that
    /// always returns `None` from `diagnostics_for`.
    lsp_manager: Arc<crate::lsp::LspManager>,
    /// Session-scoped workshop variable store (#548). Shared across all tool
    /// calls so `last_tool_result` persists within the session and can be
    /// promoted to the parent context via `promote_to_context`.
    workshop_vars: Option<
        std::sync::Arc<tokio::sync::Mutex<crate::tools::large_output_router::WorkshopVariables>>,
    >,
    /// External sandbox backend (#516). When `Some`, exec_shell routes commands
    /// through this instead of spawning a local process.
    sandbox_backend: Option<std::sync::Arc<dyn crate::sandbox::backend::SandboxBackend>>,
    /// Diagnostics collected during the current step's tool calls. Drained
    /// and forwarded as a synthetic user message before the next API call.
    pending_lsp_blocks: Vec<crate::lsp::DiagnosticBlock>,
    /// Cached SlopLedger gate block keyed by the ledger file's modified time.
    /// This keeps prompt refreshes cheap while still noticing append/update
    /// writes from slop ledger tools during the same session.
    slop_ledger_gate_cache: Option<(Option<SystemTime>, Option<String>)>,
    /// Current operating mode. Updated on `ChangeMode` and `SendMessage`.
    current_mode: AppMode,
    /// Process-local cache for `estimated_input_tokens`. Memoizes the most
    /// recent token estimate keyed on `(session.messages_revision,
    /// system_prompt_fingerprint)`. Five call sites per turn consult this
    /// (engine capacity checkpoints, seam manager, trim budget, etc.) plus
    /// four TUI / command consumers; the cache turns N×O(messages) walks
    /// into a single recompute on a content change.
    token_estimate_cache: TokenEstimateCache,
    /// Shared pause flag set by the TUI and read before tool execution.
    shared_paused: Arc<StdMutex<bool>>,
    /// Turn-loop interceptors (W3, #836). Each hook point in `turn_loop.rs`
    /// consults these; the default set is empty so existing behavior is
    /// preserved when no interceptor is registered.
    interceptors: Vec<Box<dyn crate::core::engine::interceptor::TurnInterceptor>>,
    /// Task token budget for the whole goal (#848). `None` means unbounded.
    task_budget: Option<crate::core::engine::resilience::TaskBudget>,
    /// Resume controller (#857): holds the checkpoint store + state file for
    /// the session so per-turn completion can append checkpoints and `run()`
    /// can restore prior progress. Best-effort, always present (may be empty).
    resume_controller: crate::core::engine::resilience::SharedResumeController,
    /// How many effort/model escalations have been applied via #845 retry.
    escalations_applied: u32,
    /// Whether the task token budget is exhausted (#848) — gates goal
    /// continuation and halts the run.
    budget_exhausted: bool,
    /// #855 — periodic memory consolidation scheduler. `None` disables
    /// consolidation (the field is only populated when
    /// `config.consolidation_interval_turns` is `Some`). Ticked once per
    /// completed turn; `maybe_consolidate` is guarded by `compaction_in_progress`.
    consolidation_scheduler: Option<mimofan_memory::consolidation::ConsolidationScheduler>,
    /// Latched while an auto/manual compaction is mutating the session messages
    /// so the #855 consolidation scheduler can skip its own pass and avoid
    /// contending for the storage layer (see `maybe_consolidate`'s `compacting`).
    compaction_in_progress: bool,
    /// #863 — umbrella headless gate. `Some` only when `unattended` mode is
    /// enabled; holds the resolved failure-log path and lets the engine append
    /// a structured failure event on an unrecoverable error.
    headless_gate: Option<crate::core::engine::headless_gate::HeadlessGate>,
}

// === Internal tool helpers ===

fn subagent_mailbox_message_is_best_effort(message: &MailboxMessage) -> bool {
    matches!(
        message,
        MailboxMessage::Progress { .. }
            | MailboxMessage::ToolCallStarted { .. }
            | MailboxMessage::ToolCallCompleted { .. }
    )
}

const SUBAGENT_MAILBOX_BEST_EFFORT_MIN_INTERVAL: Duration = Duration::from_millis(100);

fn subagent_mailbox_best_effort_send_permitted(
    last_sent_at: &mut HashMap<String, Instant>,
    message: &MailboxMessage,
    now: Instant,
) -> bool {
    if !subagent_mailbox_message_is_best_effort(message) {
        return true;
    }

    let agent_id = message.agent_id().to_string();
    if last_sent_at
        .get(&agent_id)
        .is_some_and(|last| now.duration_since(*last) < SUBAGENT_MAILBOX_BEST_EFFORT_MIN_INTERVAL)
    {
        return false;
    }

    last_sent_at.insert(agent_id, now);
    true
}

impl Engine {
    /// Return the sandbox backend to use for the given workspace.
    ///
    /// Overridable seam (#835): callers (and plugin assemblies) can select a
    /// per-workspace backend, but the default simply returns the engine-wide
    /// external backend configured at startup. The per-turn injection at the
    /// tool-context wiring already uses `self.sandbox_backend`, so this method
    /// exposes the same value through a stable, overridable entry point.
    #[must_use]
    pub fn sandbox_for(
        &self,
        _workspace: &str,
    ) -> Option<Arc<dyn crate::sandbox::backend::SandboxBackend>> {
        self.sandbox_backend.clone()
    }

    pub(super) async fn emit_compaction_started(
        &mut self,
        id: String,
        auto: bool,
        message: String,
    ) {
        let _ = self
            .tx_event
            .send(Event::CompactionStarted { id, auto, message })
            .await;
    }

    pub(super) async fn emit_compaction_completed(
        &mut self,
        id: String,
        auto: bool,
        message: String,
        messages_before: Option<usize>,
        messages_after: Option<usize>,
    ) {
        let _ = self
            .tx_event
            .send(Event::CompactionCompleted {
                id,
                auto,
                message,
                messages_before,
                messages_after,
            })
            .await;
    }

    pub(super) async fn emit_compaction_failed(&mut self, id: String, auto: bool, message: String) {
        let _ = self
            .tx_event
            .send(Event::CompactionFailed { id, auto, message })
            .await;
    }

    /// Spawns a background cache warmup after compaction to restore the prefix
    /// cache hit rate.  Errors are silently logged — a failed warmup should
    /// never interrupt the user's turn.
    pub(super) fn spawn_cache_warmup_after_compaction(
        &self,
        client: &ApiClient,
        messages: &[Message],
        system: Option<&SystemPrompt>,
        tools: Option<&[Tool]>,
        model: &str,
        reasoning_effort: Option<&str>,
    ) {
        use crate::client::build_cache_warmup_request;

        let request = MessageRequest {
            model: model.to_string(),
            messages: messages.to_vec(),
            max_tokens: 1024,
            system: system.cloned(),
            tools: tools.map(|t| t.to_vec()),
            tool_choice: None,
            metadata: None,
            thinking: None,
            reasoning_effort: reasoning_effort.map(str::to_string),
            stream: None,
            temperature: None,
            top_p: None,
            response_format: None,
        };
        let warmup = build_cache_warmup_request(&request);
        let client = client.clone();
        tokio::spawn(async move {
            match tokio::time::timeout(
                std::time::Duration::from_secs(45),
                client.create_message(warmup),
            )
            .await
            {
                Ok(Ok(_)) => {
                    crate::logging::info("Post-compaction cache warmup succeeded");
                }
                Ok(Err(err)) => {
                    crate::logging::info(format!("Post-compaction cache warmup failed: {err}"));
                }
                Err(_) => {
                    crate::logging::info("Post-compaction cache warmup timed out");
                }
            }
        });
    }

    fn reset_cancel_token(&mut self) {
        let token = CancellationToken::new();
        self.cancel_token = token.clone();
        match self.shared_cancel_token.lock() {
            Ok(mut shared) => {
                *shared = token;
            }
            Err(poisoned) => {
                *poisoned.into_inner() = token;
            }
        }
        // Fresh turn → clear any latched cancellation reason from the
        // previous turn so a downstream "request cancelled" message
        // doesn't inherit a stale cause.
        match self.cancel_reason.lock() {
            Ok(mut slot) => *slot = None,
            Err(poisoned) => *poisoned.into_inner() = None,
        }
        match self.shared_paused.lock() {
            Ok(mut paused) => *paused = false,
            Err(poisoned) => *poisoned.into_inner() = false,
        }
    }

    fn env_only_api_key_recovery_hint(api_config: &Config) -> Option<String> {
        if !crate::config::active_provider_uses_env_only_api_key(api_config) {
            return None;
        }

        let provider = api_config.api_provider();
        let env_var = provider.env_vars_label();

        Some(format!(
            "The rejected key came from {env_var}; no saved config key is present.\n\
             Run `mimofan auth status` to inspect credential sources, then \
             `mimofan auth set --provider {provider}` to save a valid key in ~/.mimofan/config.toml, \
             or remove the stale export and open a fresh shell.",
            provider = provider.as_str()
        ))
    }

    pub(super) fn decorate_auth_error_message(&self, message: String) -> String {
        let Some(hint) = self.api_key_env_only_recovery.as_ref() else {
            return message;
        };
        if crate::error_taxonomy::classify_error_message(&message) != ErrorCategory::Authentication
            || message.contains("no saved config key is present")
        {
            return message;
        }
        format!("{message}\n\n{hint}")
    }

    fn activate_runtime_route(&mut self, provider: ApiProvider, model: &str) -> Result<(), String> {
        if self.api_provider == provider
            && self
                .deepseek_client
                .as_ref()
                .is_some_and(|client| client.api_provider() == provider)
        {
            return Ok(());
        }

        let route =
            resolve_runtime_route(&self.api_config, provider, Some(model)).map_err(|reason| {
                format!(
                    "Failed to resolve provider route {} / {}: {reason}",
                    provider.as_str(),
                    model
                )
            })?;
        let route_config = route.config;
        match ApiClient::from_candidate(
            &route_config,
            &route.candidate,
            self.config.catalog_cache.clone(),
        ) {
            Ok(client) => {
                self.api_provider = provider;
                self.api_config = route_config;
                self.api_key_env_only_recovery =
                    Self::env_only_api_key_recovery_hint(&self.api_config);
                self.deepseek_client = Some(client.clone());
                self.deepseek_client_error = None;
                self.seam_manager = self
                    .seam_manager
                    .as_ref()
                    .filter(|manager| manager.config().enabled)
                    .map(|manager| SeamManager::new(client, manager.config().clone()));
                Ok(())
            }
            Err(err) => Err(format!(
                "Failed to configure provider route {} / {}: {err}",
                provider.as_str(),
                model
            )),
        }
    }

    /// Create a new engine with the given configuration
    pub fn new(config: EngineConfig, api_config: &Config) -> (Self, EngineHandle) {
        crate::tls::ensure_rustls_crypto_provider();

        // Capture the resume path before `config` is moved into the struct so
        // the resume controller can be built from it (#857).
        let resume_session_path = config.resume_session.clone();
        let task_budget_tokens = config.task_budget_tokens;
        // #853/#855/#863 — capture the headless/unattended + consolidation
        // config before `config` is moved into the struct.
        let unattended = config.unattended;
        let consolidation_interval_turns = config.consolidation_interval_turns;
        let failure_log_path = config.failure_log_path.clone();
        let max_steps = config.max_steps;
        let config_workspace = config.workspace.clone();

        if let Some(objective) = normalized_goal_objective(config.goal_objective.as_deref()) {
            sync_goal_state_from_host(
                &config.goal_queue,
                Some(&objective),
                config.goal_token_budget,
                config.goal_status,
            );
        }
        // 宿主同步之后兜底：仅当队列完全为空（宿主未注入任何目标）时，
        // 才用本地落盘文件恢复运行时队列。宿主注入优先，本地文件仅兜底，
        // 绝不覆盖宿主意图。best-effort：任何失败都静默跳过。
        if config
            .goal_queue
            .lock()
            .map(|q| q.is_empty())
            .unwrap_or(false)
        {
            if let Some(restored) = load_goal_queue_fallback(None) {
                if let Ok(mut q) = config.goal_queue.lock() {
                    *q = restored;
                }
            }
        }

        let (tx_op, rx_op) = mpsc::channel(32);
        let (tx_event, rx_event) = mpsc::channel(256);
        let (tx_approval, rx_approval) = mpsc::channel(64);
        let (tx_user_input, rx_user_input) = mpsc::channel(32);
        let (tx_steer, rx_steer) = mpsc::channel(64);
        let (tx_subagent_completion, rx_subagent_completion) = mpsc::unbounded_channel();
        let cancel_token = CancellationToken::new();
        let shared_cancel_token = Arc::new(StdMutex::new(cancel_token.clone()));
        let cancel_reason: Arc<StdMutex<Option<CancelReason>>> = Arc::new(StdMutex::new(None));
        let shared_paused = Arc::new(StdMutex::new(false));
        let tool_exec_lock = Arc::new(RwLock::new(()));

        // Create clients for both providers
        let (deepseek_client, deepseek_client_error) =
            match ApiClient::new(api_config, config.catalog_cache.clone()) {
                Ok(client) => (Some(client), None),
                Err(err) => (None, Some(err.to_string())),
            };
        let api_provider = api_config.api_provider();
        let api_key_env_only_recovery = Self::env_only_api_key_recovery_hint(api_config);

        let mut session = Session::new(
            config.model.clone(),
            config.workspace.clone(),
            config.allow_shell,
            config.trust_mode,
            config.notes_path.clone(),
            config.mcp_config_path.clone(),
        );
        // One-time migration of any legacy single-file memory into the
        // categorized directory layout (idempotent; no-op when absent).
        let legacy_memory = config.memory_dir.with_file_name("memory.md");
        let _ = crate::memory::migrate_legacy(&legacy_memory, &config.memory_dir);

        // Set up stable system prompt with project context (default to agent mode).
        // Per-turn working-set metadata is injected into the latest user
        // message at request time so file churn does not rewrite this prefix.
        let user_memory_block =
            crate::memory::compose_index_block(config.memory_enabled, &config.memory_dir, None);
        let prompt_goal_objective =
            goal_objective_for_prompt(config.goal_objective.as_deref(), &config.goal_queue);
        let goal_contract = crate::core::engine::goal::goal_contract_for_prompt(&config.goal_queue);
        let system_prompt =
            prompts::system_prompt_for_mode_with_context_skills_session_and_approval(
                &config.workspace,
                None,
                Some(&config.skills_dir),
                Some(&config.instructions),
                prompts::PromptSessionContext {
                    user_memory_block: user_memory_block.as_deref(),
                    goal_objective: prompt_goal_objective.as_deref(),
                    goal_completion_check: goal_contract
                        .as_ref()
                        .and_then(|c| c.completion_check.as_deref()),
                    goal_progress_checklist: goal_contract
                        .as_ref()
                        .and_then(|c| c.progress_checklist.as_deref()),
                    project_context_pack_enabled: config.project_context_pack_enabled,
                    locale_tag: &config.locale_tag,
                    translation_enabled: config.translation_enabled,
                    model_id: &config.model,
                    context_window_override: Some(
                        crate::route_budget::route_context_window_tokens(
                            api_provider,
                            &config.model,
                            config.active_route_limits,
                        ),
                    ),
                    show_thinking: config.show_thinking,
                    verbosity: config.verbosity.as_deref(),
                    skills_scan_mimofan_only: config.skills_scan_mimofan_only,
                    frozen_spec: config.frozen_spec.as_deref(),
                },
            );
        let stable_prompt = Some(system_prompt);
        session.last_system_prompt_hash = Some(system_prompt_hash(stable_prompt.as_ref()));
        session.system_prompt = stable_prompt;

        // Initialize prefix-cache stability monitor (lazy-pin).
        // The system prompt is available now but the tool catalog isn't
        // fully built until the first turn, so we start unpinned. The
        // first `check_and_update` call in the turn loop will pin the
        // fingerprint automatically.
        let _ = session.prefix_stability.get_or_insert_with(|| {
            // Use the tool registry's spec names for fingerprinting.
            // At this point tool spec builders may not be registered yet,
            // so we start with None — fingerprint will pin on first request.
            crate::prefix_cache::PrefixStabilityManager::new_unpinned()
        });

        let subagent_manager = new_shared_subagent_manager_with_timeout(
            config.workspace.clone(),
            config.max_subagents,
            config.max_admitted_subagents,
            config.subagent_heartbeat_timeout,
            config.launch_concurrency,
            config.subagent_token_budget,
        );
        let shell_manager = config
            .runtime_services
            .shell_manager
            .clone()
            .unwrap_or_else(|| new_shared_shell_manager(config.workspace.clone()));
        // Create Flash seam manager for layered context (#159). v0.7.5 keeps
        // this opt-in until the prefix-cache audit proves when seam production
        // is worth the extra request and transcript mutation.
        let seam_manager = deepseek_client.as_ref().map(|main_client| {
            let seam_config = SeamConfig {
                enabled: api_config.context.enabled.unwrap_or(false),
                verbatim_window_turns: api_config
                    .context
                    .verbatim_window_turns
                    .unwrap_or(crate::seam_manager::VERBATIM_WINDOW_TURNS),
                l1_threshold: api_config
                    .context
                    .l1_threshold
                    .unwrap_or(crate::seam_manager::DEFAULT_L1_THRESHOLD),
                l2_threshold: api_config
                    .context
                    .l2_threshold
                    .unwrap_or(crate::seam_manager::DEFAULT_L2_THRESHOLD),
                l3_threshold: api_config
                    .context
                    .l3_threshold
                    .unwrap_or(crate::seam_manager::DEFAULT_L3_THRESHOLD),
                seam_model: api_config
                    .context
                    .seam_model
                    .clone()
                    .unwrap_or_else(|| crate::seam_manager::DEFAULT_SEAM_MODEL.to_string()),
            };
            SeamManager::new(main_client.clone(), seam_config)
        });

        let lsp_manager = Arc::new(match config.lsp_config.clone() {
            Some(cfg) => crate::lsp::LspManager::new(cfg, config.workspace.clone()),
            None => crate::lsp::LspManager::disabled(),
        });

        // Workshop variable store (#548). Created unconditionally so the Arc
        // can be handed to every ToolContext; routing is gated on the router
        // field being Some rather than on the vars Arc being present.
        let workshop_vars: Option<
            std::sync::Arc<
                tokio::sync::Mutex<crate::tools::large_output_router::WorkshopVariables>,
            >,
        > = if config.workshop.is_some() {
            Some(std::sync::Arc::new(tokio::sync::Mutex::new(
                crate::tools::large_output_router::WorkshopVariables::default(),
            )))
        } else {
            None
        };

        // External sandbox backend (#516). Logged but non-fatal: if the
        // backend fails to construct, the engine continues with local
        // execution as the fallback.
        let sandbox_backend = crate::sandbox::backend::create_backend(api_config)
            .unwrap_or_else(|e| {
                tracing::warn!("Failed to create sandbox backend: {e}");
                None
            })
            .map(std::sync::Arc::from);

        let active_route_limits = config.active_route_limits;

        // ── Crash / interruption recovery (#D11.4) ──
        // On startup, inspect the durable session transcript for a turn that
        // was interrupted before completion (a dangling user prompt, or an
        // assistant message with unfulfilled tool calls). If one is found,
        // re-dispatch it as a fresh `SendMessage` so the user does not lose
        // the in-flight request. Best-effort: if the channel is full or
        // closed we fall back to logging the recovery signal and continue.
        if let Some(resumable) = crate::core::engine::recovery::resume_interrupted_turn(&session) {
            let prompt = resumable.prompt.clone();
            tracing::warn!(
                reason = %resumable.reason,
                prompt_len = prompt.len(),
                "resuming interrupted turn from durable session",
            );
            let resume_op = Op::SendMessage {
                content: prompt,
                mode: AppMode::Agent,
                provider: None,
                model: session.model.clone(),
                goal_objective: config.goal_objective.clone(),
                goal_token_budget: config.goal_token_budget,
                goal_status: config.goal_status,
                reasoning_effort: session.reasoning_effort.clone(),
                reasoning_effort_auto: session.reasoning_effort_auto,
                response_format: session.response_format.clone(),
                auto_model: false,
                allow_shell: config.allow_shell,
                trust_mode: config.trust_mode,
                auto_approve: session.auto_approve,
                approval_mode: session.approval_mode,
                translation_enabled: config.translation_enabled,
                show_thinking: config.show_thinking,
                allowed_tools: config.allowed_tools.clone(),
                dynamic_tools: Vec::new(),
                hook_executor: config.hook_executor.clone(),
                verbosity: config.verbosity.clone(),
                provenance: UserInputProvenance::Runtime,
            };
            if tx_op.try_send(resume_op).is_err() {
                tracing::warn!("could not re-enqueue interrupted turn; recovery skipped");
            }
        }

        // Fire-and-forget background refresh of the live catalog for the
        // active provider. Non-fatal: failures are recorded in the shared
        // cache (visible to the picker) and the on-disk cache is rewritten
        // best-effort. Does not block engine startup or the first turn.
        // Cloned *before* the engine takes ownership of `deepseek_client` /
        // `config` below.
        if let Some(bg_client) = deepseek_client.clone() {
            let bg_cache = config.catalog_cache.clone();
            tokio::spawn(async move {
                let _ = bg_client
                    .refresh_catalog(crate::client::CATALOG_TTL_SECS)
                    .await;
                crate::config_persistence::save_catalog_cache(&bg_cache);
            });
        }

        let engine = Engine {
            config,
            api_config: api_config.clone(),
            deepseek_client,
            deepseek_client_error,
            api_key_env_only_recovery,
            session,
            subagent_manager,
            shell_manager,
            mcp_pool: None,
            api_provider,
            active_route_limits,
            rx_op,
            tx_op: tx_op.clone(),
            rx_approval,
            rx_user_input,
            rx_steer,
            tx_event,
            tx_subagent_completion,
            rx_subagent_completion,
            delivered_subagent_completion_ids: HashSet::new(),
            cancel_token: cancel_token.clone(),
            shared_cancel_token: shared_cancel_token.clone(),
            cancel_reason: cancel_reason.clone(),
            tool_exec_lock,
            seam_manager,
            turn_counter: 0,
            #[cfg(feature = "vector-memory")]
            vector_memory_injected: false,
            #[cfg(feature = "vector-memory")]
            vector_memory_block: None,
            user_profile: crate::memory::UserProfile::load(
                crate::memory::UserProfile::default_path()
                    .unwrap_or_else(|| std::path::PathBuf::from(".mimofan/user_profile.json")),
            )
            .into_non_empty(),
            lsp_manager,
            pending_lsp_blocks: Vec::new(),
            slop_ledger_gate_cache: None,
            workshop_vars,
            sandbox_backend,
            current_mode: AppMode::Agent,
            token_estimate_cache: TokenEstimateCache::new(),
            shared_paused: shared_paused.clone(),
            interceptors: Vec::new(),
            task_budget: crate::core::engine::resilience::TaskBudget::from_config(
                task_budget_tokens,
            ),
            resume_controller: {
                use crate::core::engine::resilience::ResumeController;
                let controller = resume_session_path
                    .as_ref()
                    .map(|p| ResumeController::open_path(p.as_path()));
                crate::core::engine::resilience::SharedResumeController::new(controller)
            },
            escalations_applied: 0,
            budget_exhausted: false,
            consolidation_scheduler: consolidation_interval_turns.map(|interval| {
                mimofan_memory::consolidation::ConsolidationScheduler::with_interval(
                    interval as u64,
                )
            }),
            compaction_in_progress: false,
            headless_gate: if unattended {
                use crate::core::engine::headless_gate::{HeadlessGate, HeadlessGateConfig};
                let mut gate = HeadlessGate::new(HeadlessGateConfig {
                    unattended: true,
                    task_budget_tokens,
                    max_steps,
                    failure_log_path,
                });
                // Validate eagerly so a misconfigured headless run fails fast at
                // engine construction rather than mid-run. The workspace is the
                // engine's configured workspace.
                if let Err(err) = gate.validate(&config_workspace) {
                    tracing::warn!("Headless gate validation deferred to run(): {err}");
                }
                Some(gate)
            } else {
                None
            },
        };

        let handle = EngineHandle {
            tx_op,
            rx_event: Arc::new(RwLock::new(rx_event)),
            cancel_token: shared_cancel_token,
            cancel_reason,
            tx_approval,
            tx_user_input,
            tx_steer,
            shared_paused,
        };

        (engine, handle)
    }

    /// Register a turn-loop interceptor (W3, #836). Used by the plugin
    /// assembly layer / eval harness to inject hooks without changing
    /// `Engine::new`'s signature. Implementations are consulted at the three
    /// wrap points in `turn_loop.rs`; the default (no interceptor) preserves
    /// existing behavior.
    pub fn with_interceptor(
        &mut self,
        interceptor: Box<dyn crate::core::engine::interceptor::TurnInterceptor>,
    ) -> &mut Self {
        self.interceptors.push(interceptor);
        self
    }

    async fn handle_run_shell_command(
        &mut self,
        command: String,
        mode: AppMode,
        trust_mode: bool,
        auto_approve: bool,
        approval_mode: crate::tui::approval::ApprovalMode,
    ) {
        self.reset_cancel_token();
        self.turn_counter = self.turn_counter.saturating_add(1);

        let turn_id = format!(
            "{}{seq}",
            USER_SHELL_TOOL_ID_PREFIX,
            seq = self.turn_counter
        );
        let tool_id = turn_id.clone();
        let tool_name = "exec_shell".to_string();
        let tool_input = json!({ "command": command, "source": "user" });
        let snapshot_prompt = tool_input["command"]
            .as_str()
            .unwrap_or_default()
            .to_string();

        self.session.trust_mode = trust_mode;
        self.config.trust_mode = trust_mode;
        self.session.auto_approve = auto_approve;
        let agent_approval_mode = agent_approval_mode_for_turn(auto_approve, approval_mode);
        // Only track the Agent-mode approval — Yolo/Plan have fixed
        // approval policies that are derived from the mode itself.
        if mode == AppMode::Agent {
            self.session.approval_mode = agent_approval_mode;
        }

        let _ = self
            .tx_event
            .send(Event::TurnStarted {
                turn_id: turn_id.clone(),
            })
            .await;

        if self.config.snapshots_enabled {
            let pre_workspace = self.session.workspace.clone();
            let pre_seq = self.turn_counter;
            let pre_cap = self.config.snapshots_max_workspace_bytes;
            let pre_prompt = snapshot_prompt.clone();
            let pre_conv = self.session.messages.len();
            // Fire-and-forget: the pre-turn snapshot is a best-effort archive of
            // the pre-turn state and must not block the turn from starting.
            // This mirrors the post-turn snapshot path, which is already
            // unsupervised (engine.rs post-turn call sites).
            #[allow(clippy::let_underscore_future)]
            let _ = crate::utils::spawn_blocking_supervised("pre-turn-snapshot", move || {
                let _ = pre_turn_snapshot(
                    &pre_workspace,
                    pre_seq,
                    pre_cap,
                    Some(&pre_prompt),
                    pre_conv,
                );
            });
        }

        let _ = self
            .tx_event
            .send(Event::ToolCallStarted {
                id: tool_id.clone(),
                name: tool_name.clone(),
                input: tool_input.clone(),
            })
            .await;

        let tool_context = self.build_tool_context(mode, auto_approve);
        let registry = ToolRegistryBuilder::new()
            .with_shell_tools()
            .build(tool_context);

        let result = if mode == AppMode::Plan {
            Err(ToolError::permission_denied(
                "Tool 'exec_shell' is unavailable in Plan mode".to_string(),
            ))
        } else if !self.config.features.enabled(Feature::ShellTool) {
            Err(ToolError::not_available(
                "Tool 'exec_shell' is disabled by feature flag".to_string(),
            ))
        } else if let Some(spec) = registry.get(&tool_name) {
            let mut approval_required = spec.approval_requirement() != ApprovalRequirement::Auto
                && !registry.context().auto_approve;
            let mut approval_description = spec.description().to_string();
            let mut approval_force_prompt = false;
            let ask_rule_decision = exec_shell_ask_rule_decision(
                &self.config,
                &tool_name,
                &tool_input,
                &self.session.workspace,
                self.session.approval_mode,
            );
            if let Some(ToolAskRuleDecision::Prompt(reason)) = ask_rule_decision.as_ref() {
                // YOLO mode (auto_approve) is the explicit "no approvals"
                // contract: a typed ask-rule must not pop a modal in YOLO.
                // A typed deny rule still blocks hard below.
                if !self.session.auto_approve {
                    approval_required = true;
                    approval_description = reason.clone();
                    approval_force_prompt = true;
                }
            }
            if let Some(ToolAskRuleDecision::Block(reason)) = ask_rule_decision {
                Err(ToolError::permission_denied(reason))
            } else if approval_required {
                emit_tool_audit(json!({
                    "event": "tool.approval_required",
                    "tool_id": tool_id.clone(),
                    "tool_name": tool_name.clone(),
                    "source": "composer_bang",
                }));
                let approval_key =
                    crate::tools::approval_cache::build_approval_key(&tool_name, &tool_input).0;
                let approval_grouping_key =
                    crate::tools::approval_cache::build_approval_grouping_key(
                        &tool_name,
                        &tool_input,
                    )
                    .0;
                let _ = self
                    .tx_event
                    .send(Event::ApprovalRequired {
                        id: tool_id.clone(),
                        tool_name: tool_name.clone(),
                        input: tool_input.clone(),
                        description: approval_description,
                        approval_key,
                        approval_grouping_key,
                        intent_summary: None,
                        approval_force_prompt,
                    })
                    .await;

                match self.await_tool_approval(&tool_id).await {
                    Ok(ApprovalResult::Approved) => {
                        emit_tool_audit(json!({
                            "event": "tool.approval_decision",
                            "tool_id": tool_id.clone(),
                            "tool_name": tool_name.clone(),
                            "decision": "approved",
                            "source": "composer_bang",
                        }));
                        Self::execute_tool_with_lock(
                            self.tool_exec_lock.clone(),
                            spec.supports_parallel(),
                            false,
                            self.tx_event.clone(),
                            tool_name.clone(),
                            tool_input.clone(),
                            self.session.workspace.clone(),
                            Some(&registry),
                            None,
                            None,
                        )
                        .await
                    }
                    Ok(ApprovalResult::Denied) => {
                        emit_tool_audit(json!({
                            "event": "tool.approval_decision",
                            "tool_id": tool_id.clone(),
                            "tool_name": tool_name.clone(),
                            "decision": "denied",
                            "source": "composer_bang",
                        }));
                        Err(ToolError::permission_denied(format!(
                            "Tool '{tool_name}' denied by user"
                        )))
                    }
                    Ok(ApprovalResult::RetryWithPolicy(policy)) => {
                        emit_tool_audit(json!({
                            "event": "tool.approval_decision",
                            "tool_id": tool_id.clone(),
                            "tool_name": tool_name.clone(),
                            "decision": "retry_with_policy",
                            "policy": format!("{policy:?}"),
                            "source": "composer_bang",
                        }));
                        let elevated_context = registry
                            .context()
                            .clone()
                            .with_elevated_sandbox_policy(policy);
                        Self::execute_tool_with_lock(
                            self.tool_exec_lock.clone(),
                            spec.supports_parallel(),
                            false,
                            self.tx_event.clone(),
                            tool_name.clone(),
                            tool_input.clone(),
                            self.session.workspace.clone(),
                            Some(&registry),
                            None,
                            Some(elevated_context),
                        )
                        .await
                    }
                    Err(err) => Err(err),
                }
            } else {
                Self::execute_tool_with_lock(
                    self.tool_exec_lock.clone(),
                    spec.supports_parallel(),
                    false,
                    self.tx_event.clone(),
                    tool_name.clone(),
                    tool_input.clone(),
                    self.session.workspace.clone(),
                    Some(&registry),
                    None,
                    None,
                )
                .await
            }
        } else {
            Err(ToolError::not_available(
                "tool 'exec_shell' is not registered".to_string(),
            ))
        };

        let mut result = result;
        if let Ok(tool_result) = result.as_mut()
            && let Some(path) = crate::tools::truncate::apply_spillover_with_artifact(
                tool_result,
                &tool_id,
                &tool_name,
                &self.session.id,
            )
        {
            emit_tool_audit(json!({
                "event": "tool.spillover",
                "tool_id": tool_id.clone(),
                "tool_name": tool_name.clone(),
                "path": path.display().to_string(),
                "source": "composer_bang",
            }));
        }

        let status = if result.is_err() {
            TurnOutcomeStatus::Failed
        } else {
            TurnOutcomeStatus::Completed
        };
        let error = result.as_ref().err().map(ToString::to_string);

        let _ = self
            .tx_event
            .send(Event::ToolCallComplete {
                id: tool_id,
                name: tool_name,
                result,
            })
            .await;

        let _ = self
            .tx_event
            .send(Event::TurnComplete {
                usage: Usage::default(),
                status,
                error,
                tool_catalog: None,
                base_url: None,
            })
            .await;

        if self.config.snapshots_enabled {
            let post_workspace = self.session.workspace.clone();
            let post_seq = self.turn_counter;
            let post_cap = self.config.snapshots_max_workspace_bytes;
            let post_conv = self.session.messages.len();
            crate::utils::spawn_blocking_supervised("post-shell-turn-snapshot", move || {
                post_turn_snapshot(
                    &post_workspace,
                    post_seq,
                    post_cap,
                    Some(&snapshot_prompt),
                    post_conv,
                );
            });
        }
    }

    /// Run the engine event loop
    #[allow(clippy::too_many_lines)]
    pub async fn run(mut self) {
        enum EngineRunInput {
            Operation(Box<Op>),
            SubAgentCompletion(SubAgentCompletion),
        }

        // #857 — auto-resume: if a prior checkpoint/state file exists for this
        // session, restore orchestration state (turn index, budget, objective)
        // so the run continues from the last completed turn boundary instead
        // of restarting from scratch. Best-effort: any error is swallowed and
        // we fall back to a normal fresh start.
        self.apply_resume_on_start().await;

        loop {
            let input = tokio::select! {
                op = self.rx_op.recv() => op.map(|o| EngineRunInput::Operation(Box::new(o))),
                completion = self.rx_subagent_completion.recv() => {
                    completion.map(EngineRunInput::SubAgentCompletion)
                }
            };
            let Some(input) = input else {
                break;
            };

            match input {
                EngineRunInput::SubAgentCompletion(completion) => {
                    self.handle_idle_subagent_completion(completion).await;
                }
                EngineRunInput::Operation(op) => match *op {
                    Op::SendMessage {
                        content,
                        mode,
                        provider,
                        model,
                        goal_objective,
                        goal_token_budget,
                        goal_status,
                        reasoning_effort,
                        reasoning_effort_auto,
                        response_format,
                        auto_model,
                        allow_shell,
                        trust_mode,
                        auto_approve,
                        approval_mode,
                        translation_enabled,
                        show_thinking,
                        allowed_tools,
                        dynamic_tools,
                        hook_executor,
                        verbosity,
                        provenance,
                    } => {
                        self.handle_send_message(
                            content,
                            mode,
                            provider,
                            model,
                            goal_objective,
                            goal_token_budget,
                            goal_status,
                            reasoning_effort,
                            reasoning_effort_auto,
                            response_format,
                            auto_model,
                            allow_shell,
                            trust_mode,
                            auto_approve,
                            approval_mode,
                            translation_enabled,
                            show_thinking,
                            allowed_tools,
                            dynamic_tools,
                            hook_executor,
                            verbosity,
                            provenance,
                        )
                        .await;
                    }
                    Op::RunShellCommand {
                        command,
                        mode,
                        trust_mode,
                        auto_approve,
                        approval_mode,
                    } => {
                        self.handle_run_shell_command(
                            command,
                            mode,
                            trust_mode,
                            auto_approve,
                            approval_mode,
                        )
                        .await;
                    }
                    Op::SetGoalStatus {
                        status,
                        clear,
                        loop_config,
                    } => {
                        self.handle_set_goal_status(status, clear, loop_config)
                            .await;
                    }
                    Op::CancelRequest => {
                        self.cancel_token.cancel();
                        self.reset_cancel_token();
                    }
                    Op::ApproveToolCall { id } => {
                        // Tool approval handling will be implemented in tools module
                        let _ = self
                            .tx_event
                            .send(Event::status(format!("Approved tool call: {id}")))
                            .await;
                    }
                    Op::DenyToolCall { id } => {
                        let _ = self
                            .tx_event
                            .send(Event::status(format!("Denied tool call: {id}")))
                            .await;
                    }
                    Op::SpawnSubAgent { prompt } => {
                        let Some(client) = self.deepseek_client.clone() else {
                            let message = self
                                .deepseek_client_error
                                .as_deref()
                                .map(|err| format!("Failed to spawn sub-agent: {err}"))
                                .unwrap_or_else(|| {
                                    "Failed to spawn sub-agent: API client not configured"
                                        .to_string()
                                });
                            let _ = self
                                .tx_event
                                .send(Event::error(ErrorEnvelope::fatal(message)))
                                .await;
                            continue;
                        };

                        let mcp_pool = if self.config.features.enabled(Feature::Mcp) {
                            self.ensure_mcp_pool().await.ok()
                        } else {
                            None
                        };

                        // Share the parent manager's bus and task-claim manager
                        // with the spawned sub-agent runtime (#699).
                        let (shared_bus, shared_claims) = {
                            let guard = self.subagent_manager.read().await;
                            (Arc::clone(guard.bus()), Arc::clone(guard.task_claims()))
                        };

                        let mut runtime = SubAgentRuntime::new(
                            client,
                            self.session.model.clone(),
                            // Sub-agents don't inherit YOLO mode - use Agent mode defaults
                            self.build_tool_context(AppMode::Agent, self.session.auto_approve),
                            self.session.allow_shell,
                            Some(self.tx_event.clone()),
                            Arc::clone(&self.subagent_manager),
                        )
                        .with_role_models(self.config.subagent_model_overrides.clone())
                        .with_auto_model(self.session.auto_model)
                        .with_reasoning_effort(
                            self.session.reasoning_effort.clone(),
                            self.session.reasoning_effort_auto,
                        )
                        .with_max_spawn_depth(self.config.max_spawn_depth)
                        .with_step_api_timeout(self.config.subagent_api_timeout)
                        .with_speech_output_dir(self.config.speech_output_dir.clone())
                        .with_mcp_pool(mcp_pool)
                        .with_todos(self.config.todos.clone())
                        .with_bus(shared_bus)
                        .with_task_claims(shared_claims)
                        .background_runtime();
                        let route = resolve_subagent_assignment_route(
                            &runtime,
                            None,
                            &prompt,
                            &SubAgentType::General,
                            ModelRoute::Inherit,
                            SubAgentThinking::Inherit,
                        )
                        .await;
                        runtime.model = route.model;
                        runtime.reasoning_effort = route.reasoning_effort;
                        runtime.reasoning_effort_auto = false;

                        let result = {
                            let mut manager = self.subagent_manager.write().await;
                            manager.spawn_background(
                                Arc::clone(&self.subagent_manager),
                                runtime,
                                SubAgentType::General,
                                prompt.clone(),
                                None,
                            )
                        };

                        match result {
                            Ok(snapshot) => {
                                let _ = self
                                    .tx_event
                                    .send(Event::status(format!(
                                        "Spawned sub-agent {}",
                                        snapshot.agent_id
                                    )))
                                    .await;
                            }
                            Err(err) => {
                                let _ = self
                                    .tx_event
                                    .send(Event::error(ErrorEnvelope::fatal(format!(
                                        "Failed to spawn sub-agent: {err}"
                                    ))))
                                    .await;
                            }
                        }
                    }
                    Op::ListSubAgents => {
                        let agents = {
                            let mut manager = self.subagent_manager.write().await;
                            manager.cleanup(Duration::from_secs(60 * 60));
                            manager.list()
                        };
                        let _ = self.tx_event.send(Event::AgentList { agents }).await;
                    }
                    Op::ChangeMode { mode } => {
                        self.current_mode = mode;
                        self.emit_session_updated().await;
                        let _ = self
                            .tx_event
                            .send(Event::status(format!(
                                "Mode changed to: {}",
                                mode.description()
                            )))
                            .await;
                    }
                    Op::SetModel {
                        model,
                        mode: _,
                        route_limits,
                    } => {
                        self.session.auto_model = model.trim().eq_ignore_ascii_case("auto");
                        self.session.model = model;
                        self.config.model.clone_from(&self.session.model);
                        self.active_route_limits = route_limits;
                        self.refresh_system_prompt();
                        self.emit_session_updated().await;
                        let _ = self
                            .tx_event
                            .send(Event::status(format!(
                                "Model set to: {}",
                                self.session.model
                            )))
                            .await;
                    }
                    Op::SetCompaction { config } => {
                        let enabled = config.enabled;
                        self.config.compaction = config;
                        let _ = self
                            .tx_event
                            .send(Event::status(format!(
                                "Auto-compaction {}",
                                if enabled { "enabled" } else { "disabled" }
                            )))
                            .await;
                    }
                    Op::SetStreamChunkTimeout { timeout_secs } => {
                        self.config.stream_chunk_timeout = Duration::from_secs(timeout_secs);
                        let _ = self
                            .tx_event
                            .send(Event::status(format!(
                                "Stream chunk timeout set to {timeout_secs}s"
                            )))
                            .await;
                    }
                    Op::SetSubagentRuntimeConfig {
                        enabled,
                        max_subagents,
                        launch_concurrency,
                        max_spawn_depth,
                        api_timeout_secs,
                        heartbeat_timeout_secs,
                    } => {
                        self.config.subagents_enabled = enabled;
                        self.config.max_subagents =
                            max_subagents.clamp(1, crate::config::MAX_SUBAGENTS);
                        self.config.launch_concurrency =
                            launch_concurrency.clamp(1, self.config.max_subagents);
                        self.config.max_spawn_depth =
                            max_spawn_depth.min(mimofan_config::MAX_SPAWN_DEPTH_CEILING);
                        self.config.subagent_api_timeout = Duration::from_secs(api_timeout_secs);
                        self.config.subagent_heartbeat_timeout =
                            Duration::from_secs(heartbeat_timeout_secs);
                        let launch_gate_applied = {
                            let mut manager = self.subagent_manager.write().await;
                            manager.update_runtime_limits(
                                self.config.max_subagents,
                                self.config.max_admitted_subagents,
                                self.config.subagent_heartbeat_timeout,
                                self.config.launch_concurrency,
                                self.config.subagent_token_budget,
                            )
                        };
                        let launch_note = if launch_gate_applied {
                            ""
                        } else {
                            "; launch_concurrency takes full effect after active sub-agents finish or the session restarts"
                        };
                        let _ = self
                            .tx_event
                            .send(Event::status(format!(
                                "Sub-agent runtime updated: enabled={enabled}, max_subagents={}, launch_concurrency={}, max_depth={}{}",
                                self.config.max_subagents,
                                self.config.launch_concurrency,
                                self.config.max_spawn_depth,
                                launch_note
                            )))
                            .await;
                    }
                    Op::SyncSession {
                        session_id,
                        messages,
                        system_prompt,
                        system_prompt_override,
                        model,
                        workspace,
                    } => {
                        if let Some(session_id) = session_id {
                            self.session.id = session_id;
                        } else if messages.is_empty() && system_prompt.is_none() {
                            self.session.id = uuid::Uuid::new_v4().to_string();
                        }
                        self.session.messages = messages.into();
                        self.session.compaction_summary_prompt =
                            extract_compaction_summary_prompt(system_prompt.clone());
                        self.session.system_prompt = system_prompt;
                        self.session.last_system_prompt_hash =
                            Some(system_prompt_hash(self.session.system_prompt.as_ref()));
                        // Host-supplied prompts are persisted prefixes. Keep them
                        // byte-stable; mode/runtime state is projected per request.
                        self.session.system_prompt_override =
                            system_prompt_override && self.session.system_prompt.is_some();
                        self.session.auto_model = model.trim().eq_ignore_ascii_case("auto");
                        self.session.model = model;
                        self.session.workspace = workspace.clone();
                        self.config.model.clone_from(&self.session.model);
                        self.config.workspace = workspace.clone();
                        let ctx =
                            crate::project_context::load_project_context_with_parents(&workspace);
                        self.session.project_context = if ctx.has_instructions() {
                            Some(ctx)
                        } else {
                            None
                        };
                        self.session.rebuild_working_set();
                        self.emit_session_updated().await;
                        let _ = self
                            .tx_event
                            .send(Event::status("Session context synced".to_string()))
                            .await;
                    }
                    Op::CompactContext { instructions } => {
                        self.handle_manual_compaction(instructions).await;
                    }
                    Op::GetSessionSnapshot { tx } => {
                        let total_tokens = self.session.total_usage.input_tokens
                            + self.session.total_usage.output_tokens;
                        let snapshot = SessionSnapshot {
                            messages: self.session.messages.to_vec(),
                            total_tokens,
                            model: self.session.model.clone(),
                            workspace: self.session.workspace.clone(),
                            system_prompt: self.session.system_prompt.clone(),
                            mode: self.current_mode.as_setting().to_string(),
                        };
                        if let Some(tx) = tx.lock().ok().and_then(|mut g| g.take()) {
                            let _ = tx.send(snapshot);
                        }
                    }
                    Op::PurgeContext => {
                        self.handle_purge().await;
                    }
                    Op::EditLastTurn { new_message } => {
                        // #383: /edit — remove the last user+assistant exchange
                        // from the session, then re-send with the new content.
                        // Pop messages from the tail until we've removed the
                        // most recent user message and everything after it.
                        // First, find the last user message index.
                        let mut cut = None;
                        for (idx, msg) in self.session.messages.iter().enumerate().rev() {
                            if msg.role == "user" {
                                cut = Some(idx);
                                break;
                            }
                        }
                        if let Some(idx) = cut {
                            self.session.messages.truncate_to(idx);
                            self.session.bump_messages_revision();
                        }
                        // Now dispatch the new message as a normal send,
                        // reusing the engine's stored mode/model config.
                        let mode = AppMode::Agent; // default fallback
                        self.handle_send_message(
                            new_message,
                            mode,
                            Some(self.api_provider),
                            self.session.model.clone(),
                            self.config.goal_objective.clone(),
                            self.config.goal_token_budget,
                            self.config.goal_status,
                            self.session.reasoning_effort.clone(),
                            self.session.reasoning_effort_auto,
                            self.session.response_format.clone(),
                            self.session.auto_model,
                            self.session.allow_shell,
                            self.session.trust_mode,
                            self.session.auto_approve,
                            self.session.approval_mode,
                            self.config.translation_enabled,
                            self.config.show_thinking,
                            self.config.allowed_tools.clone(),
                            Vec::new(),
                            self.config.hook_executor.clone(),
                            self.config.verbosity.clone(),
                            UserInputProvenance::ExternalUser,
                        )
                        .await;
                    }
                    Op::Shutdown => {
                        break;
                    }
                },
            }
        }

        // #freeze: flush any sub-agent checkpoint that the hot-path debounce
        // coalesced away, so a graceful shutdown keeps the latest progress.
        {
            let mut manager = self.subagent_manager.write().await;
            manager.flush_pending_persist();
        }

        // #420: graceful MCP shutdown — send SIGTERM and give stdio servers
        // a brief window to exit before drop fires SIGKILL via kill_on_drop.
        // Best-effort: pool may not exist (no MCP configured) and the lock
        // can fail under contention; either way the kill_on_drop fallback
        // still reaps the children.
        if let Some(pool) = self.mcp_pool.as_ref() {
            let mut guard = pool.lock().await;
            guard.shutdown_all().await;
        }
    }

    // ── Engine resilience: budget / checkpoint / state / resume (#845/#848/#851/#856/#857) ──

    /// #857 — apply a prior checkpoint/state at engine start.
    ///
    /// Restores `turn_counter`, the task budget, the objective, and the
    /// escalation count from the serialized agent state. Best-effort: any
    /// failure is logged and ignored so a corrupt state file never blocks a
    /// fresh start.
    async fn apply_resume_on_start(&mut self) {
        use crate::core::engine::resilience::ResumeController;
        let Some(mut controller) = self.resume_controller.take() else {
            return;
        };
        if !controller.has_resume_point() {
            // No prior progress; keep the (empty) controller for per-turn writes.
            self.resume_controller.set(controller);
            return;
        }

        // Resume the turn counter so the next turn continues after the last
        // completed one (skipping already-done turns).
        if let Some(last_turn) = controller.resume_from_turn() {
            self.turn_counter = last_turn;
        }

        if let Some(state) = controller.load_state() {
            if let Some(remaining) = state.budget_remaining
                && let Some(budget) = self.task_budget.as_mut()
            {
                budget.remaining = remaining;
                budget.consumed = state.tokens_consumed;
                if let Some(total) = state.budget_total {
                    budget.total = total;
                }
            }
            self.escalations_applied = state.escalations_applied;
            if !state.objective.is_empty() {
                self.config.goal_objective = Some(state.objective.clone());
            }
        }

        let _ = self
            .tx_event
            .send(Event::status(format!(
                "Resumed session from turn {} (checkpoint: {})",
                self.turn_counter,
                controller.checkpoints().path().display()
            )))
            .await;

        // Hand the controller back so per-turn completion can keep appending.
        self.resume_controller.set(controller);
    }

    /// #856 — project the live engine state into a serializable view.
    #[must_use]
    pub fn snapshot_state(&self) -> crate::core::engine::resilience::SerializableAgentState {
        use crate::core::engine::resilience::SerializableAgentState;
        let mut state = SerializableAgentState {
            objective: self.config.goal_objective.clone().unwrap_or_default(),
            turn_index: self.turn_counter,
            escalations_applied: self.escalations_applied,
            model: self.session.model.clone(),
            reasoning_effort: self.session.reasoning_effort.clone(),
            tokens_consumed: self
                .session
                .total_usage
                .input_tokens
                .saturating_add(self.session.total_usage.output_tokens)
                as usize,
            ..Default::default()
        };
        if let Some(budget) = &self.task_budget {
            state.budget_remaining = Some(budget.remaining);
            state.budget_total = Some(budget.total);
        }
        // Active sub-agents are captured lazily: `SubAgentManager` is behind an
        // async RwLock, and `snapshot_state` must stay sync. Callers needing
        // the live sub-agent set can populate `active_subagents` separately.
        state
    }

    /// #856 — best-effort restore of orchestration state from a snapshot.
    ///
    /// Only the fields safe to re-apply without a live LLM are restored
    /// (objective, budget, turn index, escalations). The message transcript is
    /// owned by the session on disk and is NOT touched here.
    pub fn restore_state(
        &mut self,
        state: &crate::core::engine::resilience::SerializableAgentState,
    ) {
        if !state.objective.is_empty() {
            self.config.goal_objective = Some(state.objective.clone());
        }
        self.turn_counter = state.turn_index;
        self.escalations_applied = state.escalations_applied;
        if let (Some(remaining), Some(total)) = (state.budget_remaining, state.budget_total) {
            self.task_budget = Some(crate::core::engine::resilience::TaskBudget {
                total,
                remaining,
                consumed: state.tokens_consumed,
            });
        }
        if !state.model.is_empty() {
            self.session.model = state.model.clone();
            self.config.model.clone_from(&state.model);
        }
        self.session.reasoning_effort = state.reasoning_effort.clone();
    }

    /// #851 — persist a turn checkpoint after a completed turn.
    ///
    /// Appends to the session's `.checkpoints.jsonl` (idempotent) and, when a
    /// task budget is active, finalizes the serialized agent state so a crash
    /// can resume. Best-effort: errors are logged, not fatal.
    async fn record_turn_checkpoint(&mut self, summary: &str) {
        let objective = self.config.goal_objective.clone().unwrap_or_default();
        let tokens = self
            .session
            .total_usage
            .input_tokens
            .saturating_add(self.session.total_usage.output_tokens) as usize;
        if let Err(e) = self.resume_controller.save_turn_checkpoint(
            self.turn_counter,
            summary,
            &objective,
            tokens,
        ) {
            tracing::warn!(target: "resilience", "turn checkpoint failed: {e}");
            return;
        }
        // Persist the serializable agent state alongside the checkpoint.
        let state = self.snapshot_state();
        if let Err(e) = self.resume_controller.save_state(&state) {
            tracing::warn!(target: "resilience", "agent state persist failed: {e}");
        }
    }

    /// #848 — decrement the task budget by a turn's usage and report/halt.
    ///
    /// Returns `true` if the budget is now exhausted (caller should stop the
    /// goal loop). Best-effort: when no budget is configured this is a no-op
    /// returning `false`.
    async fn spend_turn_budget(&mut self, usage: &Usage) -> bool {
        let Some(budget) = self.task_budget.as_mut() else {
            return false;
        };
        let exhausted = budget.spend_usage(usage);
        if exhausted {
            self.budget_exhausted = true;
            let _ = self
                .tx_event
                .send(Event::status(format!(
                    "Task token budget exhausted ({} / {} tokens). Stopping goal.",
                    budget.consumed, budget.total
                )))
                .await;
        } else {
            let _ = self
                .tx_event
                .send(Event::status(budget.context_marker()))
                .await;
        }
        exhausted
    }

    /// #848 — whether the model-facing budget marker should be injected.
    #[must_use]
    pub fn budget_context_marker(&self) -> Option<String> {
        self.task_budget.as_ref().map(|b| b.context_marker())
    }

    /// #845 — validate the just-completed turn and, if it failed, escalate
    /// effort/model and re-dispatch within the escalation cap.
    ///
    /// Returns `true` if the turn should be considered validated (either it
    /// passed or escalations are exhausted), `false` if a retry was dispatched
    /// (the caller should treat the current turn as not-yet-complete).
    ///
    /// The validation uses the `GoalGate` building blocks when an objective and
    /// a success predicate / required substring are available; otherwise it is
    /// a no-op (returns `true`).
    async fn maybe_retry_on_validation_failure(&mut self, observed_output: Option<String>) -> bool {
        let Some(retry_config) = self.config.validation_retry.clone() else {
            return true;
        };
        let objective = retry_config
            .objective
            .clone()
            .or_else(|| self.config.goal_objective.clone())
            .unwrap_or_default();
        if objective.is_empty() {
            return true;
        }

        // Validate synchronously (no `!Send` `GoalGate` live across an `.await`
        // — compute the verdict and the escalation step up front, then perform
        // the channel sends afterwards so the engine future stays `Send`).
        let (verdict_met, step) = {
            use crate::tools::verifier::goal_gate::{GoalEvidence, GoalGate};
            let mut evidence = GoalEvidence {
                observed_output,
                ..Default::default()
            };
            let gate = GoalGate::default_set();
            let verdict = gate.evaluate(&objective, &evidence);
            if verdict.met {
                return true;
            }
            use crate::core::engine::resilience::EffortTier;
            let current_effort = self
                .session
                .reasoning_effort
                .as_deref()
                .map(EffortTier::parse)
                .unwrap_or(EffortTier::Medium);
            let step = retry_config.policy.escalate(
                &current_effort,
                &self.session.model,
                self.escalations_applied,
            );
            (verdict.met, step)
        };

        if !step.changed {
            return true;
        }
        self.escalations_applied = self.escalations_applied.saturating_add(1);
        self.session.reasoning_effort = Some(step.effort.as_str().to_string());
        self.session.reasoning_effort_auto = false;
        self.config.model.clone_from(&step.model);
        self.session.model.clone_from(&step.model);
        self.refresh_system_prompt();

        let _ = self
            .tx_event
            .send(Event::status(format!(
                "Validation failed; escalating (effort={}, model={}, escalations={}) and retrying turn.",
                step.effort.as_str(),
                step.model,
                self.escalations_applied
            )))
            .await;

        // Re-dispatch the same objective as a runtime continuation turn.
        let _ = self
            .tx_op
            .send(Op::SendMessage {
                content: objective.clone(),
                mode: self.current_mode,
                provider: Some(self.api_provider),
                model: self.session.model.clone(),
                goal_objective: Some(objective),
                goal_token_budget: self.config.goal_token_budget,
                goal_status: crate::tools::goal::GoalStatus::Active,
                reasoning_effort: self.session.reasoning_effort.clone(),
                reasoning_effort_auto: false,
                response_format: self.session.response_format.clone(),
                auto_model: self.session.auto_model,
                allow_shell: self.session.allow_shell,
                trust_mode: self.session.trust_mode,
                auto_approve: self.session.auto_approve,
                approval_mode: self.session.approval_mode,
                translation_enabled: self.config.translation_enabled,
                show_thinking: self.config.show_thinking,
                allowed_tools: self.config.allowed_tools.clone(),
                dynamic_tools: Vec::new(),
                hook_executor: self.config.hook_executor.clone(),
                verbosity: self.config.verbosity.clone(),
                provenance: crate::core::ops::UserInputProvenance::Runtime,
            })
            .await;
        false
    }

    #[allow(clippy::too_many_arguments)]
    async fn handle_send_message(
        &mut self,
        content: String,
        mode: AppMode,
        provider: Option<ApiProvider>,
        model: String,
        goal_objective: Option<String>,
        goal_token_budget: Option<u32>,
        goal_status: GoalStatus,
        reasoning_effort: Option<String>,
        reasoning_effort_auto: bool,
        response_format: Option<serde_json::Value>,
        auto_model: bool,
        allow_shell: bool,
        trust_mode: bool,
        auto_approve: bool,
        approval_mode: crate::tui::approval::ApprovalMode,
        translation_enabled: bool,
        show_thinking: bool,
        allowed_tools: Option<Vec<String>>,
        dynamic_tools: Vec<DynamicToolSpec>,
        hook_executor: Option<std::sync::Arc<crate::hooks::HookExecutor>>,
        verbosity: Option<String>,
        provenance: UserInputProvenance,
    ) {
        // `App::fast_mode_active` lives on the TUI side and is never sent across
        // the engine boundary, but `/fast` is exactly "pinned model + low
        // effort": it turns auto-routing off and forces `low`. Auto-routed turns
        // are excluded because the router picks an effort per message, so a
        // routed `low` is a per-turn judgement rather than a standing speed
        // preference. Computed up front because `reasoning_effort` is moved into
        // the session before the tool catalog is built.
        let fast_mode = !auto_model
            && reasoning_effort
                .as_deref()
                .is_some_and(|effort| effort.eq_ignore_ascii_case("low"));

        let input_policy = effective_input_policy(
            provenance,
            mode,
            &content,
            allow_shell,
            trust_mode,
            auto_approve,
            approval_mode,
        );
        if let Some(status) = input_policy.status.clone() {
            let _ = self.tx_event.send(Event::status(status)).await;
        }
        // Reset cancel token for fresh turn (in case previous was cancelled)
        self.reset_cancel_token();

        // #848 — a fresh user-initiated turn clears the budget-exhausted latch
        // so a new goal is not blocked by a previous goal's exhaustion.
        if provenance == crate::core::ops::UserInputProvenance::ExternalUser {
            self.budget_exhausted = false;
        }

        // Track current mode so mid-turn messages include the right mode in turn metadata.
        self.current_mode = input_policy.mode;

        // Drain stale steer messages from previous turns.
        while self.rx_steer.try_recv().is_ok() {}

        // Create turn context first so start event includes a stable turn id.
        let mut turn = TurnContext::new(self.config.max_steps);
        self.turn_counter = self.turn_counter.saturating_add(1);

        // Emit turn started event IMMEDIATELY so the UI knows the turn is
        // active. The snapshot below can take 30+ seconds on slow filesystems
        // (e.g. WSL2 /mnt/c) and must not delay the TurnStarted event.
        let _ = self
            .tx_event
            .send(Event::TurnStarted {
                turn_id: turn.id.clone(),
            })
            .await;

        // First-turn vector-memory semantic recall → system-prompt injection
        // (#570, complement to file-based `/memory`). Latched so the (network)
        // embedding recall runs at most once per session; non-fatal on any error.
        #[cfg(feature = "vector-memory")]
        {
            let query = content.clone();
            self.maybe_inject_vector_memory(&query).await;
        }

        // Snapshot the workspace BEFORE we touch a single tool. Run the git
        // work on the blocking pool so the async runtime stays responsive;
        // failure is non-fatal (the helper logs at WARN).
        if self.config.snapshots_enabled {
            // Clone the user prompt now — `content` is moved into
            // `user_text_message_with_turn_metadata_for_route` below, so we need
            // a copy for both pre- and post-turn snapshot labels. The
            // label carries a truncated first line so `/restore`
            // listings are human-readable.
            let snapshot_prompt = content.clone();
            let pre_workspace = self.session.workspace.clone();
            let pre_seq = self.turn_counter;
            let pre_cap = self.config.snapshots_max_workspace_bytes;
            let pre_conv = self.session.messages.len();
            // Fire-and-forget: archive the pre-turn state without blocking the
            // turn start (mirrors the unsupervised post-turn snapshot path).
            #[allow(clippy::let_underscore_future)]
            let _ = crate::utils::spawn_blocking_supervised("pre-turn-snapshot", move || {
                let _ = pre_turn_snapshot(
                    &pre_workspace,
                    pre_seq,
                    pre_cap,
                    Some(&snapshot_prompt),
                    pre_conv,
                );
            });
        }

        // A new turn means any leftover retry banner (success cleared
        // it, failure pinned it) is no longer relevant — reset to idle
        // so the footer doesn't display a stale failure row across
        // turns (#499).
        crate::retry_status::clear();

        // Clone user prompt for post-turn snapshot label before `content`
        // is moved into `user_text_message_with_turn_metadata_for_route` below.
        let snapshot_prompt_post = content.clone();

        // Check if we have the appropriate client
        if let Some(provider) = provider
            && let Err(message) = self.activate_runtime_route(provider, &model)
        {
            self.deepseek_client_error = Some(message.clone());
            let _ = self
                .tx_event
                .send(Event::error(ErrorEnvelope::fatal_auth(message.clone())))
                .await;
            let _ = self
                .tx_event
                .send(Event::TurnComplete {
                    usage: turn.usage.clone(),
                    status: TurnOutcomeStatus::Failed,
                    error: Some(message),
                    tool_catalog: None,
                    base_url: None,
                })
                .await;
            return;
        }

        if self.deepseek_client.is_none() {
            let message = self
                .deepseek_client_error
                .as_deref()
                .map(|err| format!("Failed to send message: {err}"))
                .unwrap_or_else(|| "Failed to send message: API client not configured".to_string());
            let _ = self
                .tx_event
                .send(Event::error(ErrorEnvelope::fatal_auth(message.clone())))
                .await;
            let _ = self
                .tx_event
                .send(Event::TurnComplete {
                    usage: turn.usage.clone(),
                    status: TurnOutcomeStatus::Failed,
                    error: Some(message),
                    tool_catalog: None,
                    base_url: None,
                })
                .await;
            return;
        }

        self.session
            .working_set
            .observe_user_message(&content, &self.session.workspace);
        let force_update_plan_first = should_force_update_plan_first(input_policy.mode, &content);

        let agent_approval_mode =
            agent_approval_mode_for_turn(input_policy.auto_approve, input_policy.approval_mode);
        self.session.auto_approve = input_policy.auto_approve;
        // Only track the Agent-mode approval — Yolo/Plan have fixed
        // approval policies that are derived from the mode itself.
        if input_policy.mode == AppMode::Agent {
            self.session.approval_mode = agent_approval_mode;
        }

        // Add user message to session
        let user_msg = self.user_text_message_with_turn_metadata_for_route_and_provenance(
            content,
            &model,
            auto_model,
            reasoning_effort.as_deref(),
            reasoning_effort_auto,
            provenance,
        );
        self.session.add_message(user_msg);

        let previous_goal_objective = self.config.goal_objective.clone();
        let previous_goal_token_budget = self.config.goal_token_budget;
        let previous_goal_status = self.config.goal_status;

        self.session.model = model;
        self.config.model.clone_from(&self.session.model);
        self.config.goal_objective = goal_objective.clone();
        self.config.goal_token_budget = goal_token_budget;
        self.config.goal_status = goal_status;
        if normalized_goal_objective(previous_goal_objective.as_deref())
            != normalized_goal_objective(goal_objective.as_deref())
            || previous_goal_token_budget != goal_token_budget
            || previous_goal_status != goal_status
        {
            sync_goal_state_from_host(
                &self.config.goal_queue,
                normalized_goal_objective(goal_objective.as_deref()).as_deref(),
                goal_token_budget,
                goal_status,
            );
            // 宿主同步之后兜底（同 Engine::new 的语义）：仅当队列完全为空时
            // 用本地落盘文件恢复，不覆盖宿主已注入的内容。best-effort。
            if self
                .config
                .goal_queue
                .lock()
                .map(|q| q.is_empty())
                .unwrap_or(false)
            {
                if let Some(restored) = load_goal_queue_fallback(None) {
                    if let Ok(mut q) = self.config.goal_queue.lock() {
                        *q = restored;
                    }
                }
            }
        }
        self.config.allowed_tools = allowed_tools;
        self.config.hook_executor = hook_executor;
        self.session.reasoning_effort = reasoning_effort;
        self.session.reasoning_effort_auto = reasoning_effort_auto;
        self.session.response_format = response_format;
        self.session.auto_model = auto_model;
        self.session.allow_shell = input_policy.allow_shell;
        self.config.allow_shell = input_policy.allow_shell;
        self.session.trust_mode = input_policy.trust_mode;
        self.config.trust_mode = input_policy.trust_mode;
        self.config.translation_enabled = translation_enabled;
        self.config.show_thinking = show_thinking;
        self.config.verbosity = verbosity;

        // Refresh stable prompt context.
        self.refresh_system_prompt();
        self.emit_session_updated().await;

        // Build tool registry and tool list for the current mode
        let todo_list = self.config.todos.clone();
        let plan_state = self.config.plan_state.clone();

        let tool_context = self.build_tool_context(input_policy.mode, input_policy.auto_approve);
        let builder = self
            .build_turn_tool_registry_builder(input_policy.mode, todo_list, plan_state)
            .with_dynamic_tools(&dynamic_tools)
            .with_extra_tools(self.config.extra_tools.0.clone());

        let subagents_available =
            self.config.subagents_enabled && self.config.features.enabled(Feature::Subagents);

        let fork_context_for_runtime = if subagents_available {
            let state = StructuredState::capture(
                input_policy.mode.label(),
                self.config.workspace.clone(),
                std::env::current_dir().ok(),
                &self.session.working_set,
                &self.config.todos,
                &self.config.plan_state,
                Some(&self.subagent_manager),
            )
            .await;
            Some(SubAgentForkContext {
                system: self.session.system_prompt.clone(),
                messages: self.messages_with_turn_metadata(),
                structured_state_block: state.to_system_block(),
                fork_turns: None,
            })
        } else {
            None
        };

        // Mailbox for structured sub-agent envelopes (#128/#130). One per
        // turn: the receiver is drained by a short-lived task that converts
        // envelopes into `Event::SubAgentMailbox` so the UI can route them
        // to the matching in-transcript card. The drainer exits naturally
        // when every cloned sender is dropped at turn-end.
        let mailbox_for_runtime = if subagents_available {
            let cancel_token = self.cancel_token.child_token();
            let (mailbox, mut receiver) = Mailbox::new(cancel_token.clone());
            let tx_event_clone = self.tx_event.clone();
            spawn_supervised(
                "subagent-mailbox-drainer",
                std::panic::Location::caller(),
                async move {
                    let mut best_effort_sent_at: HashMap<String, Instant> = HashMap::new();
                    while let Some(envelope) = receiver.recv().await {
                        let event = Event::SubAgentMailbox {
                            seq: envelope.seq,
                            message: envelope.message,
                        };
                        if let Event::SubAgentMailbox { message, .. } = &event
                            && subagent_mailbox_message_is_best_effort(message)
                        {
                            if !subagent_mailbox_best_effort_send_permitted(
                                &mut best_effort_sent_at,
                                message,
                                Instant::now(),
                            ) {
                                continue;
                            }
                            match tx_event_clone.try_send(event) {
                                Ok(()) => continue,
                                Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => continue,
                                Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => break,
                            }
                        }
                        if tx_event_clone.send(event).await.is_err() {
                            break;
                        }
                    }
                },
            );
            Some((mailbox, cancel_token))
        } else {
            None
        };

        let mcp_pool = if self.config.features.enabled(Feature::Mcp) {
            self.ensure_mcp_pool().await.ok()
        } else {
            None
        };

        // Share the parent manager's bus and task-claim manager with the
        // spawned sub-agent runtime (#699).
        let (shared_bus, shared_claims) = {
            let guard = self.subagent_manager.read().await;
            (Arc::clone(guard.bus()), Arc::clone(guard.task_claims()))
        };

        let mut tool_registry = match input_policy.mode {
            AppMode::Agent | AppMode::Yolo => {
                if subagents_available {
                    let runtime = if let Some(client) = self.deepseek_client.clone() {
                        let mut rt = SubAgentRuntime::new(
                            client,
                            self.session.model.clone(),
                            tool_context.clone(),
                            self.session.allow_shell,
                            Some(self.tx_event.clone()),
                            Arc::clone(&self.subagent_manager),
                        )
                        .with_role_models(self.config.subagent_model_overrides.clone())
                        .with_auto_model(self.session.auto_model)
                        .with_reasoning_effort(
                            self.session.reasoning_effort.clone(),
                            self.session.reasoning_effort_auto,
                        )
                        .with_max_spawn_depth(self.config.max_spawn_depth)
                        .with_step_api_timeout(self.config.subagent_api_timeout)
                        .with_speech_output_dir(self.config.speech_output_dir.clone())
                        .with_mcp_pool(mcp_pool.clone())
                        .with_todos(self.config.todos.clone())
                        .with_bus(shared_bus)
                        .with_task_claims(shared_claims)
                        .with_parent_completion_tx(self.tx_subagent_completion.clone());
                        if let Some(context) = fork_context_for_runtime.clone() {
                            rt = rt.with_fork_context(context);
                        }
                        if let Some((mailbox, cancel_token)) = mailbox_for_runtime.as_ref() {
                            rt = rt
                                .with_mailbox(mailbox.clone())
                                .with_cancel_token(cancel_token.clone());
                        }
                        Some(rt)
                    } else {
                        None
                    };
                    if let Some(subagent_runtime) = runtime {
                        Some(
                            builder
                                .with_subagent_tools(
                                    self.subagent_manager.clone(),
                                    subagent_runtime.clone(),
                                )
                                .with_workflow_tool(self.subagent_manager.clone(), subagent_runtime)
                                .build(tool_context),
                        )
                    } else {
                        tracing::warn!(
                            "Sub-agents enabled but no API client available, falling back to basic tool set"
                        );
                        Some(builder.build(tool_context))
                    }
                } else {
                    Some(builder.build(tool_context))
                }
            }
            _ => Some(builder.build(tool_context)),
        };

        // #853 — unattended safety subset. When running headless, restrict the
        // registry to read-only + auto-approved tools so the run never blocks
        // on a human approval prompt. We also skip plugin/MCP tool loading in
        // unattended mode: those are externally-supplied and may require
        // approval or perform egress, which would break the headless guarantee.
        let mut plugin_tool_names: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        if self.config.unattended {
            if let Some(ref mut tool_registry) = tool_registry {
                let policy = crate::tools::unattended::UnattendedPolicy::new(true);
                let allowed = policy.allowed_tool_names(tool_registry);
                let allowed_set: std::collections::HashSet<String> = allowed
                    .iter()
                    .map(std::string::ToString::to_string)
                    .collect();
                let registered: Vec<String> = tool_registry
                    .names()
                    .iter()
                    .map(|s| s.to_string())
                    .collect();
                for name in registered {
                    if !allowed_set.contains(&name) {
                        tool_registry.remove(&name);
                    }
                }
                tracing::info!(
                    "unattended mode: {} tool(s) permitted after safe-subset filter",
                    allowed_set.len()
                );
            }
        } else {
            // Load plugin tools from the user's tools directory and apply any
            // config.toml overrides. Explicit overrides win over auto-discovered
            // scripts with the same tool name.
            if let Some(ref mut tool_registry) = tool_registry {
                plugin_tool_names =
                    configure_plugin_tools(tool_registry, self.config.tools.as_ref());
            }
        }

        let mcp_tools = if !self.config.unattended && self.config.features.enabled(Feature::Mcp) {
            self.mcp_tools().await
        } else {
            Vec::new()
        };
        let tools = tool_registry.as_ref().map(|registry| {
            let capability = crate::model_profile::resolved_capability_profile(
                self.api_config.api_provider(),
                &self.config.model,
            );
            // `/fast` trades breadth for cost, so shrink the eagerly-loaded tool
            // surface the same way a small context window would. This only flips
            // heavyweight tools to `defer_loading` (they stay reachable via
            // tool_search and append to the catalog *tail*), so the catalog head
            // keeps the byte-stability the KV prefix cache depends on.
            let surface_budget = if fast_mode {
                crate::model_profile::ToolSurfaceBudget::Compact
            } else {
                capability.tool_surface_budget
            };
            let mut catalog = build_model_tool_catalog_with_surface(
                registry.to_api_tools_with_cache(true),
                mcp_tools,
                input_policy.mode,
                &self.config.tools_always_load,
                surface_budget,
                &registry.usage_map(),
            );
            for tool in &mut catalog {
                if plugin_tool_names.contains(&tool.name) {
                    tool.defer_loading = Some(false);
                }
            }
            filter_tool_catalog_for_gates(
                &mut catalog,
                self.config.allowed_tools.as_deref(),
                self.config.disallowed_tools.as_deref(),
            );
            catalog
        });
        let tool_catalog_for_event = tools.clone();
        let base_url_for_event = self
            .deepseek_client
            .as_ref()
            .map(|client| client.base_url().to_string());

        // Main turn loop. Catch panics here so an internal error surfaces as a
        // failed TurnComplete instead of unwinding through `engine.run()` and
        // killing the whole engine-event-loop task — which left the UI stuck
        // on "working" forever with the engine silently dead (#2583, #1269).
        use futures_util::FutureExt as _;
        let turn_result = std::panic::AssertUnwindSafe(self.handle_deepseek_turn(
            &mut turn,
            tool_registry.as_ref(),
            tools,
            input_policy.mode,
            force_update_plan_first,
            input_policy.dynamic_active_tools,
        ))
        .catch_unwind()
        .await;
        let (status, error) = match turn_result {
            Ok(outcome) => outcome,
            Err(panic) => {
                let detail = crate::utils::panic_message(&*panic);
                crate::utils::record_caught_panic("engine-event-loop", &detail);
                (
                    TurnOutcomeStatus::Failed,
                    Some(format!(
                        "The engine hit an internal error and stopped this turn: {detail}. \
                         Your session is intact — send your message again to retry. \
                         A crash report was saved to ~/.mimofan/crashes/."
                    )),
                )
            }
        };

        // #863 — on an unrecoverable error during an unattended run, append a
        // structured failure event to the configured failure log so a headless
        // supervisor can detect and restart the crash. Best-effort: failures to
        // write the log are logged but do not change the turn outcome.
        if self.headless_gate.is_some() && status != TurnOutcomeStatus::Completed {
            let message = error
                .clone()
                .unwrap_or_else(|| format!("turn ended with status {status:?} in unattended mode"));
            self.write_headless_failure("turn_failed", &message);
        }

        // Update session usage
        self.session.total_usage.add(&turn.usage);
        self.record_goal_usage_for_turn(&turn.usage, turn.elapsed());

        // Emit turn complete event — after all post-turn bookkeeping so
        // the terminal is immediately responsive when the UI receives it.
        self.emit_goal_updated().await;
        let turn_usage_for_budget = turn.usage;
        let _ = self
            .tx_event
            .send(Event::TurnComplete {
                usage: turn_usage_for_budget.clone(),
                status,
                error,
                tool_catalog: tool_catalog_for_event,
                base_url: base_url_for_event,
            })
            .await;

        // ── Engine resilience hooks (#848/#851/#845) ──────────────────────
        // 1. Decrement the task token budget by this turn's usage; halt if it
        //    is now exhausted.
        // 2. Persist a recoverable turn checkpoint so a crash can resume.
        // 3. If the turn failed objective validation, escalate effort/model and
        //    re-dispatch within the escalation cap (returns false when it did).
        if status == TurnOutcomeStatus::Completed {
            self.spend_turn_budget(&turn_usage_for_budget).await;
            self.record_turn_checkpoint("turn completed").await;

            // Derive an observed-output summary from the last assistant text
            // block (best-effort) for the validation gate.
            let observed_output = self
                .session
                .messages
                .iter()
                .rev()
                .find(|m| m.role == "assistant")
                .and_then(|m| {
                    m.content.iter().rev().find_map(|b| match b {
                        crate::models::ContentBlock::Text { text, .. } => Some(text.clone()),
                        _ => None,
                    })
                });
            if !self
                .maybe_retry_on_validation_failure(observed_output)
                .await
            {
                // A retry was dispatched; skip the normal goal continuation for
                // this turn so we don't double-dispatch.
                return;
            }
        }

        // Post-turn snapshot. Fire-and-forget: TurnComplete is already
        // emitted, so the UI is unblocked and the user can type / select /
        // paste immediately (#234). The git work proceeds on the blocking
        // pool without forcing the engine loop to await it.
        if self.config.snapshots_enabled {
            // `snapshot_prompt_post` was cloned from `content` above,
            // before `content` was moved into the session messages.
            let post_workspace = self.session.workspace.clone();
            let post_seq = self.turn_counter;
            let post_cap = self.config.snapshots_max_workspace_bytes;
            let post_conv = self.session.messages.len();
            crate::utils::spawn_blocking_supervised("post-turn-snapshot", move || {
                post_turn_snapshot(
                    &post_workspace,
                    post_seq,
                    post_cap,
                    Some(&snapshot_prompt_post),
                    post_conv,
                );
            });
        }

        // ── Cross-turn goal continuation ───────────────────────────────────
        // If the turn completed successfully and a goal is still Active (and
        // under any optional budget), re-dispatch a synthetic continuation
        // message back into the engine's own op channel. This makes `/goal` a
        // persistent loop that runs until the model self-reports complete or
        // blocked, the user pauses/clears, or an optional budget is exhausted.
        // There is no continuation cap. A Failed or Interrupted turn does NOT
        // continue — Esc cancels the loop by interrupting the turn.
        if status == TurnOutcomeStatus::Completed
            && !self.budget_exhausted
            && let Some(continuation) = self.goal_continuation_if_active()
        {
            // Re-dispatch with the same route/mode/approval settings as
            // the prior turn. The non-Copy values were moved into
            // `self.config` / `self.session` earlier in this function, so
            // we clone them back out here.
            let _ = self
                .tx_op
                .send(Op::SendMessage {
                    content: continuation,
                    mode,
                    provider,
                    model: self.session.model.clone(),
                    goal_objective: None,
                    goal_token_budget: None,
                    goal_status: GoalStatus::Active,
                    reasoning_effort: self.session.reasoning_effort.clone(),
                    reasoning_effort_auto,
                    response_format: self.session.response_format.clone(),
                    auto_model,
                    allow_shell,
                    trust_mode,
                    auto_approve,
                    approval_mode,
                    translation_enabled,
                    show_thinking,
                    allowed_tools: self.config.allowed_tools.clone(),
                    dynamic_tools: dynamic_tools.clone(),
                    hook_executor: self.config.hook_executor.clone(),
                    verbosity: self.config.verbosity.clone(),
                    provenance: UserInputProvenance::Runtime,
                })
                .await;
        }

        // ── #855 — periodic memory consolidation ──────────────────────────
        // Tick the consolidation scheduler once per completed turn. When the
        // interval elapses and no compaction is in progress, run a consolidation
        // pass (dedup/rollup of the memory store). The `run` callback is a
        // best-effort hook: it emits a structured checkpoint event and reports
        // whether a pass ran. Real memory-store consolidation is a follow-up;
        // this wires the previously-dead scheduler into the turn boundary.
        self.tick_consolidation().await;

        // ── idle LSP handle unload ────────────────────────────────────────
        // Per completed turn, drop any per-language LSP transport idle longer
        // than `[lsp].idle_unload_secs`. Cheap, lock-scoped, and safe: only the
        // cached `Arc` is released; in-flight requests hold their own clone.
        self.lsp_manager.maybe_unload_idle().await;
    }

    /// #855 — advance the periodic consolidation scheduler by one completed
    /// turn and trigger a consolidation pass when the interval elapses.
    ///
    /// Skipped while a compaction is mutating the session (the `in_progress`
    /// guard is driven by `compaction_in_progress`, set around the auto/manual
    /// compaction blocks) so the two never contend for the storage layer. The
    /// `run` callback is best-effort: it records a checkpoint event and returns
    /// whether a pass ran, keeping the scheduler wired without coupling the
    /// engine to a specific memory-store flush implementation.
    async fn tick_consolidation(&mut self) {
        let Some(scheduler) = self.consolidation_scheduler.as_mut() else {
            return;
        };
        // Resolve the vector-memory directory the same way
        // `maybe_inject_vector_memory` does, so the consolidation pass can reach
        // the long-term store. `None` means vector memory is unavailable here.
        let mem_dir = self.config.memory_dir.parent().map(|p| p.to_path_buf());
        Self::run_consolidation_tick(scheduler, self.compaction_in_progress, mem_dir);
    }

    /// Pure, testable core of [`Engine::tick_consolidation`]: advance the
    /// scheduler by one turn and run a consolidation pass when the interval
    /// elapses. Extracted so the wiring can be unit-tested without constructing
    /// a full [`Engine`]. Returns `Some(true)` when a consolidation pass ran,
    /// `Some(false)` when skipped (compacting), `None` before the interval.
    ///
    /// `memory_dir` is the vector-store directory (or `None`). When present and
    /// the store is enabled, the pass actually enforces the capacity policy so
    /// low-retention observations are evicted and the long-term store stays
    /// bounded (#716 M4). Errors are best-effort: logged and swallowed.
    pub(crate) fn run_consolidation_tick(
        scheduler: &mut mimofan_memory::consolidation::ConsolidationScheduler,
        compacting: bool,
        memory_dir: Option<std::path::PathBuf>,
    ) -> Option<bool> {
        scheduler.tick();
        scheduler.maybe_consolidate(
            || compacting,
            move || {
                // Best-effort consolidation pass: evict low-retention vector
                // memories when the store is reachable, then record a
                // checkpoint so a headless supervisor and the event stream
                // observe the pass.
                if let Some(dir) = memory_dir {
                    if let Ok(mut vm) = crate::vector_memory::VectorMemory::open(&dir) {
                        if vm.enabled() {
                            if let Err(err) = vm
                                .enforce_capacity_policy(mimofan_memory::vector::VectorStore::DEFAULT_CAPACITY_LIMIT)
                            {
                                tracing::warn!(
                                    "vector-memory capacity enforcement failed, skipping: {err}"
                                );
                            }
                        }
                    }
                }
                true
            },
        )
    }

    /// Build the consolidation scheduler from config, if enabled. Extracted so
    /// the `config → scheduler` wiring is unit-testable without a full engine.
    pub(crate) fn build_consolidation_scheduler(
        config: &EngineConfig,
    ) -> Option<mimofan_memory::consolidation::ConsolidationScheduler> {
        config.consolidation_interval_turns.map(|interval| {
            mimofan_memory::consolidation::ConsolidationScheduler::with_interval(interval as u64)
        })
    }

    /// #863 — append a structured failure event to the headless failure log.
    ///
    /// No-op when unattended mode is off (no gate configured). Best-effort:
    /// any I/O error is logged via `tracing` and swallowed so a logging failure
    /// never masks the original engine error.
    fn write_headless_failure(&self, code: &str, message: &str) {
        let Some(gate) = self.headless_gate.as_ref() else {
            return;
        };
        if let Err(err) = gate.write_failure(code, message) {
            tracing::warn!("failed to write headless failure event: {err}");
        } else {
            tracing::info!("wrote headless failure event [{code}]: {message}");
        }
    }

    async fn handle_manual_compaction(&mut self, instructions: Option<String>) {
        let id = format!("compact_{}", &uuid::Uuid::new_v4().to_string()[..8]);
        let zero_usage = Usage {
            input_tokens: 0,
            output_tokens: 0,
            ..Usage::default()
        };
        let Some(client) = self.deepseek_client.clone() else {
            let message = "Manual compaction unavailable: API client not configured".to_string();
            self.emit_compaction_failed(id, false, message.clone())
                .await;
            let _ = self
                .tx_event
                .send(Event::error(ErrorEnvelope::fatal_auth(message.clone())))
                .await;
            let _ = self
                .tx_event
                .send(Event::TurnComplete {
                    usage: zero_usage,
                    status: TurnOutcomeStatus::Failed,
                    error: Some(message),
                    tool_catalog: None,
                    base_url: None,
                })
                .await;
            return;
        };

        // `/compact <instructions>` wins over the project's persistent
        // `# Compact Instructions` section for this one run; with no inline
        // argument the persistent section (already resolved into the config)
        // still applies.
        let mut compaction_config = self.config.compaction.clone();
        if let Some(inline) = instructions
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty())
        {
            compaction_config.custom_instructions = Some(inline.to_string());
        }

        let start_message = match compaction_config.custom_instructions.as_deref() {
            Some(text) => format!("Manual context compaction started (focus: {text})"),
            None => "Manual context compaction started".to_string(),
        };
        self.emit_compaction_started(id.clone(), false, start_message)
            .await;

        let compaction_pins = self
            .session
            .working_set
            .pinned_message_indices(&self.session.messages, &self.session.workspace);
        let compaction_paths = self.session.working_set.top_paths(24);
        let messages_before = self.session.messages.len();
        let mut turn_status = TurnOutcomeStatus::Completed;
        let mut turn_error = None;

        match compact_messages_safe_with_objective(
            &client,
            &self.session.messages,
            &compaction_config,
            Some(&self.session.workspace),
            Some(&compaction_pins),
            Some(&compaction_paths),
            None,
            self.config.goal_self_check_after_compact,
        )
        .await
        {
            Ok(result) => {
                if !result.messages.is_empty() || self.session.messages.is_empty() {
                    let messages_after = result.messages.len();
                    self.session.messages = result.messages.into();
                    self.merge_compaction_summary(result.summary_prompt);
                    // Post-compaction goal self-check nudge, injected over the
                    // system channel so it never pollutes the user conversation
                    // history (see turn_loop for the equivalent auto path).
                    if let Some(loop_break) = result.self_check_nudge {
                        let nudge_block = crate::models::SystemBlock {
                            block_type: "text".to_string(),
                            text: loop_break.nudge,
                            cache_control: None,
                        };
                        let merged = crate::compaction::merge_system_prompts(
                            self.session.system_prompt.as_ref(),
                            Some(crate::models::SystemPrompt::Blocks(vec![nudge_block])),
                        );
                        self.session.system_prompt = merged;
                        self.session.last_system_prompt_hash =
                            Some(system_prompt_hash(self.session.system_prompt.as_ref()));
                    }
                    self.emit_session_updated().await;
                    let removed = messages_before.saturating_sub(messages_after);
                    let message = if result.retries_used > 0 {
                        format!(
                            "Compaction complete: {messages_before} → {messages_after} messages ({removed} removed, {} retries)",
                            result.retries_used
                        )
                    } else {
                        format!(
                            "Compaction complete: {messages_before} → {messages_after} messages ({removed} removed)"
                        )
                    };
                    self.emit_compaction_completed(
                        id,
                        false,
                        message,
                        Some(messages_before),
                        Some(messages_after),
                    )
                    .await;
                    // Warm up prefix cache so the next request doesn't cold-start.
                    self.spawn_cache_warmup_after_compaction(
                        &client,
                        &self.session.messages,
                        self.session.system_prompt.as_ref(),
                        None,
                        &self.session.model,
                        None,
                    );
                } else {
                    let message = "Compaction skipped: produced empty result".to_string();
                    self.emit_compaction_failed(id, false, message.clone())
                        .await;
                    turn_status = TurnOutcomeStatus::Failed;
                    turn_error = Some(message);
                }
            }
            Err(err) => {
                let message = format!("Manual context compaction failed: {err}");
                self.emit_compaction_failed(id, false, message.clone())
                    .await;
                let _ = self.tx_event.send(Event::status(message.clone())).await;
                turn_status = TurnOutcomeStatus::Failed;
                turn_error = Some(message);
            }
        }

        let _ = self
            .tx_event
            .send(Event::TurnComplete {
                usage: zero_usage,
                status: turn_status,
                error: turn_error,
                tool_catalog: None,
                base_url: None,
            })
            .await;
    }

    async fn handle_purge(&mut self) {
        let zero_usage = Usage {
            input_tokens: 0,
            output_tokens: 0,
            ..Usage::default()
        };
        let Some(client) = self.deepseek_client.clone() else {
            let message = "Purge unavailable: API client not configured".to_string();
            emit_purge_failed(&self.tx_event, message.clone()).await;
            let _ = self
                .tx_event
                .send(Event::error(ErrorEnvelope::fatal_auth(message.clone())))
                .await;
            let _ = self
                .tx_event
                .send(Event::TurnComplete {
                    usage: zero_usage,
                    status: TurnOutcomeStatus::Failed,
                    error: Some(message),
                    tool_catalog: None,
                    base_url: None,
                })
                .await;
            return;
        };

        emit_purge_started(
            &self.tx_event,
            "Agent context purge in progress\u{2026}".to_string(),
        )
        .await;
        let messages_before = self.session.messages.len();

        let (status, error) = match run_purge(
            &client,
            &self.session.messages,
            &self.session.model,
            self.session.reasoning_effort.clone(),
            effective_max_output_tokens_for_route(&self.session.model, self.active_route_limits),
        )
        .await
        {
            Ok(result) => {
                let messages_after = result.messages.len();
                self.session.messages = result.messages.into();
                self.emit_session_updated().await;

                let summary = format!(
                    "Purge complete: {messages_before} → {messages_after} messages \
                         ({} removed, {} condensed)",
                    result.removed_count, result.replaced_count,
                );
                emit_purge_completed(
                    &self.tx_event,
                    messages_before,
                    messages_after,
                    result.removed_count,
                    result.replaced_count,
                    summary,
                )
                .await;
                (TurnOutcomeStatus::Completed, None)
            }
            Err(e) => {
                emit_purge_failed(&self.tx_event, e.clone()).await;
                (TurnOutcomeStatus::Failed, Some(e))
            }
        };

        let _ = self
            .tx_event
            .send(Event::TurnComplete {
                usage: zero_usage,
                status,
                error,
                tool_catalog: None,
                base_url: None,
            })
            .await;
    }


    fn build_tool_context(&self, mode: AppMode, auto_approve: bool) -> ToolContext {
        // Load the per-workspace trusted-paths list (#29) on every tool-context
        // build. Cheap (a small JSON file) and always reflects the latest
        // `/trust add` / `/trust remove` mutations without an explicit cache
        // refresh hook.
        let trusted = crate::workspace_trust::WorkspaceTrust::load_for(&self.session.workspace);
        let mut trusted_external_paths = trusted.paths().to_vec();
        let clipboard_images_dir =
            crate::tui::clipboard::clipboard_images_dir(&self.session.workspace);
        if !trusted_external_paths
            .iter()
            .any(|path| path == &clipboard_images_dir)
        {
            trusted_external_paths.push(clipboard_images_dir);
        }
        let mut ctx = ToolContext::with_auto_approve(
            self.session.workspace.clone(),
            self.session.trust_mode,
            self.session.notes_path.clone(),
            self.session.mcp_config_path.clone(),
            mode == AppMode::Yolo || auto_approve,
        )
        .with_state_namespace(self.session.id.clone())
        .with_session_id(self.session.id.clone())
        .with_features(self.config.features.clone())
        .with_shell_manager(self.shell_manager.clone())
        .with_runtime_services(self.config.runtime_services.clone())
        .with_skills_config(
            self.config.skills_dir.clone(),
            self.config.skills_scan_mimofan_only,
        )
        .with_session_objects(crate::rlm::session::SessionObjectSnapshot::new(
            self.session.id.clone(),
            self.session.model.clone(),
            self.session.workspace.clone(),
            self.session.system_prompt.clone(),
            self.session.messages.clone().into(),
        ))
        .with_cancel_token(self.cancel_token.clone())
        .with_shell_policy(shell_policy_for_mode(mode, self.session.allow_shell))
        .with_trusted_external_paths(trusted_external_paths)
        .with_follow_symlinks(self.config.workspace_follow_symlinks);

        // Hand the user-memory path to tools so the model-callable
        // `remember` tool can append entries (#489). `None` when the
        // feature is disabled — tools short-circuit on that.
        if self.config.memory_enabled {
            ctx.memory_dir = Some(self.config.memory_dir.clone());
        }

        if let Some(decider) = self.config.network_policy.as_ref() {
            ctx = ctx.with_network_policy(decider.clone());
        }

        // Wire the large-output router (#548). Only attaches when the
        // [workshop] config table is present; sub-agents don't inherit the
        // router (their ToolContext is built separately) to prevent recursive
        // routing of the synthesis call itself.
        if let Some(workshop_cfg) = self.config.workshop.as_ref()
            && let Some(vars_arc) = self.workshop_vars.as_ref()
        {
            let router =
                crate::tools::large_output_router::LargeOutputRouter::new(workshop_cfg.clone());
            ctx = ctx.with_large_output_router(router, vars_arc.clone());
        }

        // Wire the external sandbox backend (#516). exec_shell checks this
        // field and routes commands through the backend instead of spawning
        // a local process when it's set.
        if let Some(backend) = self.sandbox_backend.as_ref() {
            ctx = ctx.with_sandbox_backend(std::sync::Arc::clone(backend));
        }

        // Wire search provider config.
        ctx.search_provider = self.config.search_provider;
        ctx.search_api_key = self.config.search_api_key.clone();
        ctx.search_base_url = self.config.search_base_url.clone();

        let policy = sandbox_policy_for_mode(mode, &self.session.workspace);
        let mut ctx = ctx.with_elevated_sandbox_policy(policy);
        if matches!(mode, AppMode::Plan) {
            ctx = ctx.with_shell_network_denied_hint(
                "Shell command blocked: Plan mode runs shell commands in a read-only sandbox — no writes, no network. Use Agent mode (`/mode agent`) for any command that creates or modifies files, or that needs network access.",
            );
        }
        ctx
    }

    async fn ensure_mcp_pool(&mut self) -> Result<Arc<AsyncMutex<McpPool>>, ToolError> {
        if let Some(pool) = self.mcp_pool.as_ref() {
            return Ok(Arc::clone(pool));
        }
        let mut pool = McpPool::from_config_path_with_workspace(
            &self.session.mcp_config_path,
            &self.session.workspace,
        )
        .map_err(|e| ToolError::execution_failed(format!("Failed to load MCP config: {e}")))?;
        if let Some(decider) = self.config.network_policy.as_ref() {
            pool = pool.with_network_policy(decider.clone());
        }
        let pool = Arc::new(AsyncMutex::new(pool));
        self.mcp_pool = Some(Arc::clone(&pool));
        Ok(pool)
    }

    async fn mcp_tools(&mut self) -> Vec<Tool> {
        let pool = match self.ensure_mcp_pool().await {
            Ok(pool) => pool,
            Err(err) => {
                let _ = self.tx_event.send(Event::status(format!("{err:#}"))).await;
                return Vec::new();
            }
        };

        let mut pool = pool.lock().await;
        let errors = pool.connect_all().await;
        for (server, err) in errors {
            let _ = self
                .tx_event
                .send(Event::status(format!(
                    "Failed to connect MCP server '{server}': {err:#}"
                )))
                .await;
        }

        pool.to_api_tools()
    }

    /// Handle a turn using the DeepSeek API.
    #[allow(clippy::too_many_lines)]
    /// Run the pre-request layered-context checkpoint (#159). Checks whether
    /// the active input estimate has crossed a soft-seam threshold and, if so,
    /// produces an `<archived_context>` block via Flash and appends it as an
    /// assistant message. Called from `handle_deepseek_turn` before each API
    /// request so the model always has the latest navigation aids.
    async fn layered_context_checkpoint(&mut self) {
        let Some(seam_mgr) = &self.seam_manager else {
            return;
        };
        if !seam_mgr.config().enabled {
            return;
        }

        // Compute the estimated token count *before* taking a long-lived
        // `&SeamManager` borrow — `estimated_input_tokens` mutates the
        // engine's token-estimate cache, which would conflict.
        let estimated_tokens = self.estimated_input_tokens();
        let seam_mgr = self.seam_manager.as_ref().expect("checked above");
        let highest = seam_mgr.highest_level().await;
        let Some(level) = seam_mgr.seam_level_for(estimated_tokens, highest) else {
            return;
        };

        // Determine the message range to summarize: everything before the
        // verbatim window. The verbatim window (last ~16 turns) stays
        // untouched so the model always has ground-truth recent context.
        let msg_count = self.session.messages.len();
        let verbatim_start = seam_mgr.verbatim_window_start(msg_count);
        if verbatim_start == 0 {
            return; // Not enough messages to summarize.
        }

        let msg_range_end = verbatim_start;
        let pinned = self
            .session
            .working_set
            .pinned_message_indices(&self.session.messages, &self.session.workspace);

        let _ = self
            .tx_event
            .send(Event::status(format!(
                "⏻ producing L{level} context seam ({msg_range_end} messages)…"
            )))
            .await;

        // If we have existing seams, recompact; otherwise produce fresh.
        let existing_seams = seam_mgr.collect_seam_texts(&self.session.messages).await;
        let seam_text = if existing_seams.is_empty() {
            match seam_mgr
                .produce_soft_seam(
                    &self.session.messages,
                    level,
                    0,
                    msg_range_end,
                    Some(&self.session.workspace),
                    &pinned,
                )
                .await
            {
                Ok(text) => text,
                Err(err) => {
                    crate::logging::warn(format!("L{level} soft seam failed: {err}"));
                    return;
                }
            }
        } else {
            let recent: Vec<&Message> = (0..msg_range_end)
                .filter_map(|i| self.session.messages.get(i))
                .collect();
            match seam_mgr
                .recompact(&existing_seams, &recent, level, 0, msg_range_end)
                .await
            {
                Ok(text) => text,
                Err(err) => {
                    crate::logging::warn(format!("L{level} recompact failed: {err}"));
                    return;
                }
            }
        };

        if seam_text.is_empty() {
            return;
        }

        // Capture seam count before the mutable borrow below.
        let seam_count = seam_mgr.seam_count().await;

        // Append the seam as an assistant message. This is an append-only
        // operation — no messages are deleted. The prefix cache stays hot.
        self.add_session_message(Message {
            role: "assistant".to_string(),
            content: vec![ContentBlock::Text {
                text: seam_text,
                cache_control: None,
            }],
        })
        .await;

        let _ = self
            .tx_event
            .send(Event::status(format!(
                "⏻ L{level} seam complete ({seam_count} total, {msg_range_end} messages covered)"
            )))
            .await;
    }
    /// Refresh the stable system prompt based on current non-mode context.
    fn refresh_system_prompt(&mut self) {
        let active_paths = self.session.working_set.top_paths(48);
        let user_memory_block = crate::memory::compose_index_block(
            self.config.memory_enabled,
            &self.config.memory_dir,
            Some(&active_paths),
        );
        let prompt_goal_objective = goal_objective_for_prompt(
            self.config.goal_objective.as_deref(),
            &self.config.goal_queue,
        );
        let goal_contract =
            crate::core::engine::goal::goal_contract_for_prompt(&self.config.goal_queue);
        let base = prompts::system_prompt_for_mode_with_context_skills_session_and_approval(
            &self.config.workspace,
            None,
            Some(&self.config.skills_dir),
            Some(&self.config.instructions),
            prompts::PromptSessionContext {
                user_memory_block: user_memory_block.as_deref(),
                goal_objective: prompt_goal_objective.as_deref(),
                goal_completion_check: goal_contract
                    .as_ref()
                    .and_then(|c| c.completion_check.as_deref()),
                goal_progress_checklist: goal_contract
                    .as_ref()
                    .and_then(|c| c.progress_checklist.as_deref()),
                project_context_pack_enabled: self.config.project_context_pack_enabled,
                locale_tag: &self.config.locale_tag,
                translation_enabled: self.config.translation_enabled,
                model_id: &self.config.model,
                context_window_override: Some(crate::route_budget::route_context_window_tokens(
                    self.api_provider,
                    &self.config.model,
                    self.active_route_limits,
                )),
                show_thinking: self.config.show_thinking,
                verbosity: self.config.verbosity.as_deref(),
                skills_scan_mimofan_only: self.config.skills_scan_mimofan_only,
                frozen_spec: self.config.frozen_spec.as_deref(),
            },
        );
        let mut stable_prompt =
            merge_system_prompts(Some(&base), self.session.compaction_summary_prompt.clone());

        // SlopLedger completion-gate: inject unresolved slop entries into the
        // system prompt so the agent can autonomously review them before
        // claiming the task is done (#2127).
        let gate_block = self.slop_ledger_gate_block();
        if let Some(ref block) = gate_block
            && let Some(SystemPrompt::Text(prompt_text)) = &mut stable_prompt
        {
            prompt_text.push_str("\n\n");
            prompt_text.push_str(block);
        }

        // Re-append the recalled vector-memory block (if any) so a context
        // refresh doesn't silently drop the first-turn injection (#570).
        #[cfg(feature = "vector-memory")]
        if let Some(ref block) = self.vector_memory_block
            && let Some(SystemPrompt::Text(prompt_text)) = &mut stable_prompt
        {
            prompt_text.push_str("\n\n");
            prompt_text.push_str(block);
        }

        // #732 slice C: inject the cross-session UserProfile into the system
        // prompt so the model "remembers" stable user preferences/constraints.
        // Skipped when no profile exists (keeps the prompt byte-stable and
        // prefix-cache friendly). Rendered with a token budget so a large
        // profile cannot blow the context window.
        if let Some(ref profile) = self.user_profile
            && let Some(SystemPrompt::Text(prompt_text)) = &mut stable_prompt
        {
            let block = crate::memory::inject_user_profile(profile);
            if !block.is_empty() {
                prompt_text.push_str("\n\n");
                prompt_text.push_str(&block);
            }
        }

        // #848 — expose the remaining task token budget to the model so an
        // unattended run can pace itself. Injected only when a budget is
        // configured; the marker is a plain HTML comment so it carries no
        // semantic weight for non-budgeted sessions.
        if let Some(ref marker) = self.budget_context_marker()
            && let Some(SystemPrompt::Text(prompt_text)) = &mut stable_prompt
        {
            prompt_text.push_str("\n\n");
            prompt_text.push_str(marker);
        }

        // #brain.md-inspired: inject the most recent durable decisions so a
        // fresh session starts with settled choices + their rationale in
        // context. File-only (no embedding call); no-op when memory is
        // disabled or `decisions.md` is empty. Bounded to the latest N to
        // protect the context window.
        if let Some(block) = crate::memory::compose_decision_block(
            self.config.memory_enabled,
            &self.config.memory_dir,
            8,
        )
            && let Some(SystemPrompt::Text(prompt_text)) = &mut stable_prompt
        {
            prompt_text.push_str("\n\n");
            prompt_text.push_str(&block);
        }

        let stable_hash = system_prompt_hash(stable_prompt.as_ref());
        if self.session.system_prompt_override {
            return;
        }
        if self.session.last_system_prompt_hash != Some(stable_hash) {
            self.session.system_prompt = stable_prompt;
            self.session.last_system_prompt_hash = Some(stable_hash);
        }
    }

    /// Attempt a one-time vector-memory semantic recall and inject the result
    /// into the stable system prompt. Complements the file-based `/memory`
    /// block (#570). Latched via `vector_memory_injected` so the embedding
    /// network call runs at most once per session. Any failure is logged and
    /// swallowed — vector memory is an enhancement, never a hard dependency.
    #[cfg(feature = "vector-memory")]
    pub(crate) async fn maybe_inject_vector_memory(&mut self, query: &str) {
        if self.vector_memory_injected {
            return;
        }
        self.vector_memory_injected = true;

        let mem_dir = match self.config.memory_dir.parent() {
            Some(p) => p.to_path_buf(),
            None => return,
        };
        let mut vm = match crate::vector_memory::VectorMemory::open(&mem_dir) {
            Ok(vm) => vm,
            Err(err) => {
                tracing::warn!("vector-memory open failed, skipping injection: {err}");
                return;
            }
        };
        if !vm.enabled() {
            return;
        }

        let project = self
            .session
            .workspace
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("default")
            .to_string();

        // Hold only the Send embedding service across the await; the non-Send
        // store is queried synchronously afterwards.
        let embedder = match vm.take_embedder() {
            Some(embedder) => embedder,
            None => return,
        };
        let embedding = match embedder.embed_text(query).await {
            Ok(embedding) => embedding,
            Err(err) => {
                tracing::warn!("vector-memory injection embed failed: {err}");
                return;
            }
        };
        let matches = match vm.search_embedded(&embedding, Some(&project), 8) {
            Ok(matches) => matches,
            Err(err) => {
                tracing::warn!("vector-memory injection recall failed: {err}");
                return;
            }
        };
        let block =
            match crate::vector_memory::VectorMemory::format_injection_block(&project, &matches) {
                Some(block) => block,
                None => return,
            };

        self.vector_memory_block = Some(block.clone());
        if let Some(sp) = self.session.system_prompt.as_mut() {
            match sp {
                SystemPrompt::Text(text) => {
                    text.push_str("\n\n");
                    text.push_str(&block);
                }
                SystemPrompt::Blocks(blocks) => blocks.push(SystemBlock {
                    block_type: "text".into(),
                    text: block,
                    cache_control: None,
                }),
            }
        }
    }

    fn slop_ledger_gate_block(&mut self) -> Option<String> {
        let modified = crate::slop_ledger::SlopLedger::default_path()
            .ok()
            .and_then(|path| std::fs::metadata(path).ok())
            .and_then(|metadata| metadata.modified().ok());

        if let Some((cached_modified, cached_block)) = &self.slop_ledger_gate_cache
            && *cached_modified == modified
        {
            return cached_block.clone();
        }

        let loaded = crate::slop_ledger::SlopLedger::load()
            .ok()
            .and_then(|ledger| {
                if ledger.has_open_entries() {
                    ledger.completion_gate_summary()
                } else {
                    None
                }
            });
        self.slop_ledger_gate_cache = Some((modified, loaded.clone()));
        loaded
    }

    /// Merge a compaction summary into the system prompt.
    ///
    /// **Zone affiliation (#2264)**: this mutates the system prompt, which is
    /// part of the `PinnedPrefix` zone in the three-zone contract. Compaction
    /// is the one intentional mid-session prefix mutation — the engine
    /// intentionally accepts the cache-invalidation cost because the
    /// context-reduction benefit outweighs it.
    fn merge_compaction_summary(&mut self, summary_prompt: Option<SystemPrompt>) {
        if summary_prompt.is_none() {
            return;
        }
        self.session.compaction_summary_prompt = merge_system_prompts(
            self.session.compaction_summary_prompt.as_ref(),
            summary_prompt.clone(),
        );
        let merged = merge_system_prompts(self.session.system_prompt.as_ref(), summary_prompt);
        self.session.last_system_prompt_hash = Some(system_prompt_hash(merged.as_ref()));
        self.session.system_prompt = merged;
    }
}

fn system_prompt_hash(prompt: Option<&SystemPrompt>) -> u64 {
    let mut hasher = DefaultHasher::new();
    match prompt {
        Some(SystemPrompt::Text(text)) => {
            0u8.hash(&mut hasher);
            text.hash(&mut hasher);
        }
        Some(SystemPrompt::Blocks(blocks)) => {
            1u8.hash(&mut hasher);
            for block in blocks {
                block.block_type.hash(&mut hasher);
                block.text.hash(&mut hasher);
                if let Some(cache_control) = &block.cache_control {
                    cache_control.cache_type.hash(&mut hasher);
                }
            }
        }
        None => {
            2u8.hash(&mut hasher);
        }
    }
    hasher.finish()
}

/// Spawn the engine in a background task
pub fn spawn_engine(config: EngineConfig, api_config: &Config) -> EngineHandle {
    let (engine, handle) = Engine::new(config, api_config);

    spawn_supervised(
        "engine-event-loop",
        std::panic::Location::caller(),
        async move {
            engine.run().await;
        },
    );

    handle
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MockApprovalEvent {
    Approved {
        id: String,
    },
    Denied {
        id: String,
    },
    RetryWithPolicy {
        id: String,
        policy: crate::sandbox::SandboxPolicy,
    },
}

mod approval;
mod catalog_filter;
mod context;
mod context_recovery;
pub mod engine_config;
mod engine_messages;
mod goal;
mod handle;
pub(crate) mod headless_gate;
mod plugin_tools;
mod policy;
#[path = "engine/recovery.rs"]
mod recovery;
pub mod resilience;

pub(crate) use context::compact_tool_result_for_context;
use context::{
    MAX_CONTEXT_RECOVERY_ATTEMPTS, MIN_RECENT_MESSAGES_TO_KEEP, context_input_budget_for_route,
    effective_max_output_tokens_for_route, extract_compaction_summary_prompt,
    is_context_length_error_message, summarize_text,
};
pub use engine_config::EngineConfig;

pub(crate) mod circuit_breaker;
mod dispatch;
pub mod interceptor;
mod lsp_hooks;
pub(crate) mod recovery_stats;
mod streaming;
mod token_estimate_cache;
mod tool_catalog;
mod tool_execution;
mod tool_setup;
mod trace;
mod turn_loop;
pub(crate) use token_estimate_cache::TokenEstimateCache;

pub(super) use crate::config::MAX_PARALLEL_SHELL_EXEC;
pub(crate) use catalog_filter::{default_active_native_tool_names, filter_tool_catalog_for_gates};

use self::approval::{ApprovalDecision, ApprovalResult, UserInputDecision};
use crate::tools::goal::load_goal_queue_fallback;
use self::goal::{
    goal_objective_for_prompt, normalized_goal_objective, sync_goal_state_from_host,
};
use self::plugin_tools::configure_plugin_tools;
use self::policy::{
    AutoReviewPlanDecision, ToolAskRuleDecision, agent_approval_mode_for_turn,
    auto_review_plan_decision, auto_review_run_origin_for_plan, effective_input_policy,
    exec_shell_ask_rule_decision, file_tool_ask_rule_decision,
};

use self::dispatch::{
    ParallelToolResult, ParallelToolResultEntry, ToolExecGuard, ToolExecOutcome,
    ToolExecutionBatch, ToolExecutionPlan, caller_allowed_for_tool, caller_type_for_tool_use,
    final_tool_input, format_tool_error, mcp_tool_approval_description, mcp_tool_is_parallel_safe,
    mcp_tool_is_read_only, parse_parallel_tool_calls, parse_tool_input,
    plan_tool_execution_batches, should_force_update_plan_first, should_stop_after_plan_tool,
};

use self::streaming::{
    ContentBlockKind, FAKE_WRAPPER_NOTICE, MAX_STREAM_ERRORS_BEFORE_FAIL, MAX_STREAM_RETRIES,
    MAX_TRANSPARENT_STREAM_RETRIES, STREAM_MAX_CONTENT_BYTES, STREAM_MAX_DURATION_SECS,
    ToolUseState, contains_fake_tool_wrapper, filter_tool_call_delta, should_resume_after_sleep,
    should_transparently_retry_stream, sleep_gap_detected, stream_read_error_user_message,
};
use self::tool_catalog::{
    CODE_EXECUTION_TOOL_NAME, JS_EXECUTION_TOOL_NAME, MULTI_TOOL_PARALLEL_NAME,
    REQUEST_USER_INPUT_NAME, active_tools_for_step, build_model_tool_catalog_with_surface,
    ensure_advanced_tooling, execute_code_execution_tool, execute_tool_search,
    initial_active_tools, is_tool_search_tool, maybe_hydrate_requested_deferred_tool,
    missing_tool_error_message, tool_catalog_consistency_issues,
};

use self::tool_execution::emit_tool_audit;
use self::tool_setup::{sandbox_policy_for_mode, shell_policy_for_mode};
use crate::tools::js_execution::execute_js_execution_tool;
use crate::tools::plan::EXIT_PLAN_MODE_NAME;

/// Tests for the `sandbox_for` overridable seam (#835).
///
/// Note: `Engine` has no test-only constructor and `Engine::new` requires a
/// fully-formed `Config` plus live API-client construction, so we exercise the
/// seam at the contract level — proving that `Engine::sandbox_for`'s return
/// type (`Option<Arc<dyn SandboxBackend>>`) accepts a pluggable backend and
/// that the trait is `dyn`-compatible (matching `AssembledCapabilities`).
#[cfg(test)]
mod sandbox_seam_tests {
    use std::sync::Arc;

    use crate::sandbox::backend::{SandboxBackend, SandboxOutput};
    use async_trait::async_trait;
    use std::collections::HashMap;

    struct RecordingBackend {
        ran: std::sync::Mutex<Vec<String>>,
    }

    #[async_trait]
    impl SandboxBackend for RecordingBackend {
        async fn exec(
            &self,
            cmd: &str,
            _env: &HashMap<String, String>,
        ) -> anyhow::Result<SandboxOutput> {
            self.ran.lock().unwrap().push(cmd.to_string());
            Ok(SandboxOutput {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: 0,
            })
        }
    }

    #[test]
    fn sandbox_for_return_type_accepts_pluggable_backend() {
        // Mirrors exactly what `Engine::sandbox_for` returns: an
        // `Option<Arc<dyn SandboxBackend>>`. This proves the seam is
        // dyn-compatible and that a plugin-assembled backend (e.g.
        // `AssembledCapabilities.sandbox`) composes with `Engine::sandbox_for`.
        let backend: Option<Arc<dyn SandboxBackend>> = Some(Arc::new(RecordingBackend {
            ran: std::sync::Mutex::new(Vec::new()),
        }));
        assert!(backend.is_some());

        // `None` is the default for an engine with no configured backend.
        let none_backend: Option<Arc<dyn SandboxBackend>> = None;
        assert!(none_backend.is_none());
    }
}

/// #855 — prove the consolidation scheduler is wired to the turn boundary:
/// `tick()` advances the counter each completed turn and `maybe_consolidate`
/// triggers a pass once the configured interval elapses. We exercise the exact
/// production wiring (`Engine::build_consolidation_scheduler` +
/// `Engine::run_consolidation_tick`) without constructing a full `Engine`.
#[cfg(test)]
mod consolidation_wiring_tests {
    use super::*;
    use mimofan_memory::consolidation::ConsolidationScheduler;

    #[test]
    fn config_without_interval_yields_no_scheduler() {
        let config = EngineConfig {
            consolidation_interval_turns: None,
            ..EngineConfig::default()
        };
        assert!(Engine::build_consolidation_scheduler(&config).is_none());
    }

    #[test]
    fn config_with_interval_yields_scheduler() {
        let config = EngineConfig {
            consolidation_interval_turns: Some(3),
            ..EngineConfig::default()
        };
        let scheduler = Engine::build_consolidation_scheduler(&config);
        assert!(scheduler.is_some());
        assert_eq!(scheduler.unwrap().turn_count(), 0);
    }

    #[test]
    fn tick_advances_counter_and_triggers_at_interval() {
        let mut scheduler = ConsolidationScheduler::with_interval(3);
        // Turns 1..=2: before interval, no consolidation.
        assert_eq!(Engine::run_consolidation_tick(&mut scheduler, false, None), None);
        assert_eq!(scheduler.turn_count(), 1);
        assert_eq!(Engine::run_consolidation_tick(&mut scheduler, false, None), None);
        assert_eq!(scheduler.turn_count(), 2);
        // Turn 3: interval reached -> consolidation runs.
        assert_eq!(
            Engine::run_consolidation_tick(&mut scheduler, false, None),
            Some(true)
        );
        assert_eq!(scheduler.turn_count(), 3);
        // Immediately after, interval not yet reached again.
        assert_eq!(Engine::run_consolidation_tick(&mut scheduler, false, None), None);
    }

    #[test]
    fn tick_skips_consolidation_while_compacting() {
        let mut scheduler = ConsolidationScheduler::with_interval(2);
        Engine::run_consolidation_tick(&mut scheduler, false, None);
        // Turn 2 with compaction in progress -> skipped (not run), counter still advances.
        assert_eq!(
            Engine::run_consolidation_tick(&mut scheduler, true, None),
            Some(false)
        );
        assert_eq!(scheduler.turn_count(), 2);
        // After compaction ends, the next interval triggers normally.
        assert_eq!(Engine::run_consolidation_tick(&mut scheduler, false, None), None);
        assert_eq!(
            Engine::run_consolidation_tick(&mut scheduler, false, None),
            Some(true)
        );
    }
}
