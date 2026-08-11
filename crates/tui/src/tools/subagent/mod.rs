//! Sub-agent spawning system.
//!
//! Provides tools to spawn background sub-agents, query their status,
//! and retrieve results. Sub-agents run with a filtered toolset and
//! inherit the workspace configuration from the main session.
//!
//! The model-facing surface is the single `agent` tool. Older lifecycle
//! structs and manager helpers remain executable for persisted records and
//! internal recovery while the durable runtime is reused by the new surface.

use std::collections::{HashMap, VecDeque};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::{Mutex, RwLock, Semaphore};

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use futures_util::stream::{FuturesUnordered, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::{sync::mpsc, task::JoinHandle};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::client::ApiClient;
use crate::config::MAX_SUBAGENTS;
use crate::core::events::Event;
use crate::dependencies::{ExternalTool, Git};
use crate::llm_client::LlmClient;
use crate::models::{
    ContentBlock, Message, MessageRequest, MessageResponse, SystemPrompt, Tool, Usage,
};
use crate::request_tuning::RequestTuning;
use crate::tools::handle::VarHandle;
use crate::tools::plan::{PlanState, SharedPlanState};
use crate::tools::registry::{ToolRegistry, ToolRegistryBuilder};
use crate::tools::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec,
};
use crate::tools::todo::SharedTodoList;
use crate::utils::normalize_path_components;

use crate::tui::app::ReasoningEffort;
use crate::utils::spawn_supervised;
use crate::worker_profile::{ModelRoute, ToolScope, WorkerRuntimeProfile};

pub mod aggregator;
pub mod bus;
pub mod custom_agents;
pub mod decomposer;
pub mod events;
pub mod helpers;
pub mod mailbox;
pub mod naming;
pub mod persistence;
pub mod runtime;
pub mod task_claim;
pub mod tool;
pub mod types;
#[allow(unused_imports)]
pub(crate) use helpers::*;
pub mod manager;
#[allow(unused_imports)]
pub(crate) use manager::*;
pub mod runner;
#[allow(unused_imports)]
pub(crate) use runner::*;
pub mod parser;
pub use bus::AgentBus;
#[allow(unused_imports)]
pub use custom_agents::CustomAgentRegistry;
pub use mailbox::{Mailbox, MailboxMessage};
pub use naming::{assign_unique_whale_name, whale_name_for_id};
#[allow(unused_imports)]
pub(crate) use parser::*;
pub use task_claim::{
    ClaimResult, SharedTaskClaimManager, TaskClaim, TaskClaimManager, TaskClaimStatus,
    new_shared_task_claim_manager,
};
pub use tool::AgentTool;
pub use types::{
    AgentRunArtifactRef, AgentRunFollowUpTarget, AgentRunRecommendedAction, AgentRunTakeoverTarget,
    AgentRunUsage, AgentRunVerificationSummary, AgentWorkerEvent, AgentWorkerRecord,
    AgentWorkerSpec, AgentWorkerStatus, AgentWorkerToolProfile, DEFAULT_MAX_SPAWN_DEPTH,
    PersistedSubAgent, PersistedSubAgentState, SubAgentAssignment, SubAgentCheckpoint,
    SubAgentCompletion, SubAgentForkContext, SubAgentNeedsInput, SubAgentResult, SubAgentStatus,
    SubAgentType, is_false,
};

// === Constants ===

/// Global ownership table for cache-aware resident file sub-agents (#529).
/// Maps file path → agent id. Agents hold a lease on a file while running;
/// the lease is released when the agent reaches a terminal state.
static RESIDENT_LEASES: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashMap<String, String>>,
> = std::sync::OnceLock::new();

/// Release all resident file leases held by `agent_id`. Called when an
/// agent transitions to a terminal state (completed, failed, cancelled).
#[derive(Debug, Clone, Default)]
pub(crate) struct SubAgentSpawnOptions {
    pub name: Option<String>,
    pub model: Option<String>,
    pub model_route: Option<ModelRoute>,
    pub nickname: Option<String>,
    pub fork_context: bool,
    pub token_budget: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SubAgentModelStrength {
    Same,
    Faster,
}

impl SubAgentModelStrength {
    fn parse(value: &str) -> Result<Self, ToolError> {
        let normalized = value.trim().to_ascii_lowercase();
        match normalized.as_str() {
            "same" | "inherit" | "parent" | "current" => Ok(Self::Same),
            "faster" | "fast" | "smaller" | "small" | "lower" | "cheap" | "flash" => {
                Ok(Self::Faster)
            }
            _ => Err(ToolError::invalid_input(
                "model_strength must be one of: same, faster".to_string(),
            )),
        }
    }

    fn model_route(self) -> ModelRoute {
        match self {
            Self::Same => ModelRoute::Inherit,
            Self::Faster => ModelRoute::Faster,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SubAgentThinking {
    Inherit,
    Auto,
    Effort(ReasoningEffort),
}

impl SubAgentThinking {
    fn parse(value: &str) -> Result<Self, ToolError> {
        let normalized = value.trim().to_ascii_lowercase();
        match normalized.as_str() {
            "inherit" | "parent" | "same" | "current" => Ok(Self::Inherit),
            "auto" | "automatic" => Ok(Self::Auto),
            "off" | "disabled" | "none" | "false" => Ok(Self::Effort(ReasoningEffort::Off)),
            "low" | "minimal" => Ok(Self::Effort(ReasoningEffort::Low)),
            "medium" | "mid" => Ok(Self::Effort(ReasoningEffort::Medium)),
            "high" => Ok(Self::Effort(ReasoningEffort::High)),
            "max" | "maximum" | "xhigh" | "ultracode" => Ok(Self::Effort(ReasoningEffort::Max)),
            _ => Err(ToolError::invalid_input(
                "thinking must be one of: inherit, auto, off, low, medium, high, max".to_string(),
            )),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SubAgentInput {
    text: String,
    interrupt: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct SpawnRequest {
    session_name: Option<String>,
    prompt: String,
    agent_type: SubAgentType,
    assignment: SubAgentAssignment,
    allowed_tools: Option<Vec<String>>,
    model: Option<String>,
    model_strength: SubAgentModelStrength,
    thinking: SubAgentThinking,
    /// Custom agent definition loaded from Markdown file (if applicable).
    custom_agent_def: Option<custom_agents::CustomAgentDef>,
    /// Optional working directory for the child. Must canonicalize to a path
    /// inside the parent's workspace. For first-class git worktree isolation,
    /// use `worktree` instead of pre-creating a cwd by hand.
    cwd: Option<PathBuf>,
    /// Optional first-class git worktree isolation. When set, mimofan
    /// creates a sibling worktree/branch and runs the child from that checkout.
    worktree: Option<SubAgentWorktreeRequest>,
    /// Optional file path for cache-aware resident mode (#529). When set,
    /// the child's prompt is prefixed with the file contents for prefix-cache
    /// locality. A global ownership table prevents two agents from holding
    /// a resident lease on the same file simultaneously.
    resident_file: Option<String>,
    /// When true, seed the child with the parent's system prompt and message
    /// prefix before appending the child task.
    fork_context: bool,
    /// Legacy recursion budget for descendants. The model-facing child tool
    /// surface is leaf-only; this remains for persisted/internal records.
    max_depth: Option<u32>,
    /// Optional aggregate token budget for this child and its descendants.
    /// When unset, the child inherits the parent's budget pool or the
    /// configured root default.
    token_budget: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SubAgentWorktreeRequest {
    branch: Option<String>,
    path: Option<PathBuf>,
    base_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AgentUsageBudgetScope {
    scope_id: String,
    limit: u64,
    spent: u64,
    remaining: u64,
}

/// Runtime configuration for spawning sub-agents.
///
/// Carries everything a child needs to (a) build its own tool registry —
/// including the manager so grandchildren can spawn — and (b) cooperate with
/// lifecycle cancellation and depth caps. `child_runtime()` links cancellation
/// tokens, while `background_runtime()` deliberately detaches long-running
/// `agent` sessions from the caller's turn token.
#[derive(Clone)]
pub struct SubAgentRuntime {
    pub client: ApiClient,
    pub model: String,
    pub auto_model: bool,
    pub reasoning_effort: Option<String>,
    pub reasoning_effort_auto: bool,
    pub role_models: HashMap<String, String>,
    pub context: ToolContext,
    pub allow_shell: bool,
    /// Capability contract inherited by descendants. `agent` derives a
    /// child profile from this before registering the worker record so parent,
    /// sub-agent, and fleet projections share one worker contract.
    pub worker_profile: WorkerRuntimeProfile,
    pub event_tx: Option<mpsc::Sender<Event>>,
    /// Manager handle so children can recurse via `agent`. All agents
    /// at every depth share the same manager.
    pub manager: SharedSubAgentManager,
    /// Depth in the spawn tree. 0 = top-level user turn; 1 = direct child;
    /// etc. Children clone the parent runtime and increment this on spawn.
    pub spawn_depth: u32,
    /// Agent id that should be recorded as parent for any child spawned
    /// through this runtime's model-visible `agent` tool. `None` for the
    /// root engine; set to the running sub-agent id for nested spawns so UI
    /// surfaces can render the tree.
    pub parent_agent_id: Option<String>,
    /// Hard cap on recursion depth. A child whose `spawn_depth + 1` would
    /// exceed this is rejected at the spawn entry. Use `>` (strictly
    /// greater than) so equality is allowed — matches codex's pattern.
    pub max_spawn_depth: u32,
    /// Cooperative cancellation token. Direct `child_runtime()` callers derive
    /// a child token from the parent; model-visible `agent` uses
    /// `background_runtime()` to replace that token with a detached one.
    pub cancel_token: CancellationToken,
    /// Structured progress / lifecycle stream. Cloned across children so the
    /// whole spawn tree publishes into one ordered, fan-out-able mailbox.
    /// `None` only when no consumer is wired (legacy entry points / tests).
    pub mailbox: Option<Mailbox>,
    /// Wakeup channel for this runtime's immediate parent (issue #756). For
    /// the engine's direct children this points at the engine turn loop. While
    /// a sub-agent is running, its tool registry swaps this for a local inbox
    /// so nested children report to their orchestrating sub-agent instead of
    /// flooding the root parent. `None` when no consumer is wired (tests /
    /// legacy paths).
    pub parent_completion_tx: Option<mpsc::UnboundedSender<SubAgentCompletion>>,
    /// Snapshot of the request prefix visible to an opt-in forked child.
    pub fork_context: Option<SubAgentForkContext>,
    /// The parent's MCP pool if available.
    pub mcp_pool: Option<std::sync::Arc<tokio::sync::Mutex<crate::mcp::McpPool>>>,
    /// Per-step DeepSeek API timeout for the child's `create_message` call.
    /// Resolved from `[subagents] api_timeout_secs` (clamped to 1..=1800) at
    /// engine construction so a slow but legitimate model turn does not
    /// false-timeout the child mid-thinking. `child_runtime()` and
    /// `background_runtime()` preserve the parent's value (#1806, #1808).
    pub step_api_timeout: Duration,
    /// Wall-clock budget for a single tool execution within a sub-agent step.
    /// Defaults to `DEFAULT_TOOL_TIMEOUT`; the engine may override it so a long
    /// but legitimate tool run is not killed mid-flight. `child_runtime()`
    /// preserves the parent's value.
    pub tool_timeout: Duration,
    /// Default directory for Xiaomi MiMo speech/TTS tool outputs inherited by
    /// child registries. Keeps parent and sub-agent `speech` / `tts` tools on
    /// the same `[speech].output_dir` / env override.
    pub speech_output_dir: Option<PathBuf>,
    /// Shared todo list — the parent's `SharedTodoList`, cloned into each
    /// child so sub-agent `checklist_update` calls are visible in the
    /// Work sidebar live. Without this, each child gets a fresh isolated
    /// list and the parent never sees child progress until completion.
    pub todos: SharedTodoList,
    /// Shared agent bus for inter-agent pub/sub communication and shared
    /// state. All agents spawned by the same manager share this instance.
    pub bus: Option<Arc<AgentBus>>,
    /// Shared task-claim manager, attached from the parent manager so
    /// sub-agents can coordinate exclusive task ownership (#699).
    pub task_claims: Option<SharedTaskClaimManager>,
    /// When this runtime was spawned with `worktree: true`, the isolated
    /// worktree path created for the child. `None` for non-worktree spawns.
    /// The manager uses this to `git worktree remove` the checkout on agent
    /// teardown so sub-agent worktrees do not leak (#691).
    pub worktree_path: Option<PathBuf>,
}

impl SubAgentRuntime {
    /// Create a top-level runtime configuration for sub-agent execution.
    /// Use this from the engine when constructing the runtime that the
    /// parent's tool registry passes through. Children should derive their
    /// runtime via `Self::child_runtime` instead.
    #[must_use]
    pub fn new(
        client: ApiClient,
        model: String,
        context: ToolContext,
        allow_shell: bool,
        event_tx: Option<mpsc::Sender<Event>>,
        manager: SharedSubAgentManager,
    ) -> Self {
        Self {
            client,
            model,
            auto_model: false,
            reasoning_effort: None,
            reasoning_effort_auto: false,
            role_models: HashMap::new(),
            context,
            allow_shell,
            worker_profile: WorkerRuntimeProfile::for_role(SubAgentType::General),
            event_tx,
            manager,
            spawn_depth: 0,
            parent_agent_id: None,
            max_spawn_depth: DEFAULT_MAX_SPAWN_DEPTH,
            cancel_token: CancellationToken::new(),
            mailbox: None,
            parent_completion_tx: None,
            fork_context: None,
            mcp_pool: None,
            step_api_timeout: DEFAULT_STEP_API_TIMEOUT,
            tool_timeout: DEFAULT_TOOL_TIMEOUT,
            speech_output_dir: None,
            todos: crate::tools::todo::new_shared_todo_list(),
            bus: None,
            task_claims: None,
            worktree_path: None,
        }
    }

    /// Attach the parent's shared todo list so sub-agent `checklist_update`
    /// calls are visible in the Work sidebar live. Without this, children
    /// get a fresh isolated list.
    #[must_use]
    pub fn with_todos(mut self, todos: SharedTodoList) -> Self {
        self.todos = todos;
        self
    }

    /// Attach the shared agent bus for inter-agent pub/sub communication.
    #[must_use]
    pub fn with_bus(mut self, bus: Arc<AgentBus>) -> Self {
        self.bus = Some(bus);
        self
    }

    /// Attach the shared task-claim manager so this runtime's sub-agents can
    /// claim exclusive ownership of work items (#699).
    #[must_use]
    pub fn with_task_claims(mut self, claims: SharedTaskClaimManager) -> Self {
        self.task_claims = Some(claims);
        self
    }

    /// Attach an MCP pool so the subagent can execute MCP tools.
    #[must_use]
    pub fn with_mcp_pool(
        mut self,
        pool: Option<std::sync::Arc<tokio::sync::Mutex<crate::mcp::McpPool>>>,
    ) -> Self {
        self.mcp_pool = pool;
        self
    }

    /// Override the per-step DeepSeek API timeout (default
    /// `DEFAULT_STEP_API_TIMEOUT`). Called by the engine after reading
    /// `[subagents] api_timeout_secs`. Tests may use this to fail fast
    /// without waiting the legacy 120 seconds (#1806, #1808).
    #[must_use]
    pub fn with_step_api_timeout(mut self, timeout: Duration) -> Self {
        self.step_api_timeout = timeout;
        self
    }

    /// Preserve the configured speech output directory for sub-agent tools.
    #[must_use]
    pub fn with_speech_output_dir(mut self, output_dir: Option<PathBuf>) -> Self {
        self.speech_output_dir = output_dir;
        self
    }

    /// Attach the wakeup channel for this runtime's immediate parent. The
    /// engine uses this for direct children; running sub-agents replace it in
    /// the runtime handed to their nested `agent` tool so child completions are
    /// routed back to the sub-agent that spawned them.
    #[must_use]
    pub fn with_parent_completion_tx(
        mut self,
        tx: mpsc::UnboundedSender<SubAgentCompletion>,
    ) -> Self {
        self.parent_completion_tx = Some(tx);
        self
    }

    /// Attach the current parent request prefix for `fork_context` spawns.
    #[must_use]
    pub fn with_fork_context(mut self, context: SubAgentForkContext) -> Self {
        self.fork_context = Some(context);
        self
    }

    /// Attach a `Mailbox` so this runtime and its derived children publish
    /// structured `MailboxMessage` envelopes alongside the legacy `Event`
    /// stream. Pair with [`Self::with_cancel_token`] when the mailbox close
    /// token should match this runtime's cancellation token.
    #[must_use]
    pub fn with_mailbox(mut self, mailbox: Mailbox) -> Self {
        self.mailbox = Some(mailbox);
        self
    }

    /// Replace the cancellation token (e.g. when the engine constructs the
    /// runtime alongside a mailbox bound to the same token).
    #[must_use]
    pub fn with_cancel_token(mut self, token: CancellationToken) -> Self {
        self.cancel_token = token;
        self
    }

    /// Override the maximum spawn depth (default `DEFAULT_MAX_SPAWN_DEPTH`).
    /// Used by config wiring (`[subagents] max_depth = N`) and tests.
    #[must_use]
    pub fn with_max_spawn_depth(mut self, max: u32) -> Self {
        self.max_spawn_depth = max;
        self
    }

    /// Attach raw role/type model overrides. Values are intentionally
    /// validated at spawn time so bad config fails before a partial spawn.
    #[must_use]
    pub fn with_role_models(mut self, role_models: HashMap<String, String>) -> Self {
        self.role_models = role_models;
        self
    }

    /// Preserve whether the parent session is using per-turn model routing.
    #[must_use]
    pub fn with_auto_model(mut self, auto_model: bool) -> Self {
        self.auto_model = auto_model;
        self
    }

    /// Preserve the parent's thinking configuration. Child model strength is
    /// explicit on the `agent` call; this only controls reasoning effort.
    #[must_use]
    pub fn with_reasoning_effort(
        mut self,
        reasoning_effort: Option<String>,
        reasoning_effort_auto: bool,
    ) -> Self {
        self.reasoning_effort = reasoning_effort;
        self.reasoning_effort_auto = reasoning_effort_auto;
        self
    }

    /// Return a child runtime that is deliberately detached from the parent
    /// turn cancellation token. Background sub-agents should keep running when
    /// the parent turn is cancelled; explicit agent cancellation still
    /// aborts their task handles through the manager.
    #[must_use]
    pub fn background_runtime(&self) -> Self {
        let mut runtime = self.child_runtime();
        let token = CancellationToken::new();
        runtime.cancel_token = token.clone();
        runtime.context.cancel_token = Some(token);
        runtime
    }

    /// Build a child runtime cloning this one, incrementing `spawn_depth`,
    /// and deriving a child cancellation token. Used at spawn entry to
    /// construct the runtime the new sub-agent will see.
    ///
    /// Children inherit the parent's approval state. A non-auto parent can
    /// still delegate read-only investigation, but approval-gated child tools
    /// are blocked by the sub-agent registry instead of being silently run
    /// without a prompt.
    #[must_use]
    pub fn child_runtime(&self) -> Self {
        let mut child_context = self.context.clone();
        child_context.auto_approve = self.context.auto_approve;
        Self {
            client: self.client.clone(),
            model: self.model.clone(),
            auto_model: self.auto_model,
            reasoning_effort: self.reasoning_effort.clone(),
            reasoning_effort_auto: self.reasoning_effort_auto,
            role_models: self.role_models.clone(),
            context: child_context,
            allow_shell: self.allow_shell,
            worker_profile: self.worker_profile.clone(),
            event_tx: self.event_tx.clone(),
            manager: self.manager.clone(),
            spawn_depth: self.spawn_depth + 1,
            parent_agent_id: self.parent_agent_id.clone(),
            max_spawn_depth: self.max_spawn_depth,
            cancel_token: self.cancel_token.child_token(),
            mailbox: self.mailbox.clone(),
            parent_completion_tx: self.parent_completion_tx.clone(),
            fork_context: self.fork_context.clone(),
            mcp_pool: self.mcp_pool.clone(),
            step_api_timeout: self.step_api_timeout,
            tool_timeout: self.tool_timeout,
            speech_output_dir: self.speech_output_dir.clone(),
            todos: self.todos.clone(),
            bus: self.bus.clone(),
            task_claims: self.task_claims.clone(),
            // A child runtime does NOT inherit the parent's worktree path: each
            // `worktree: true` spawn sets its own path in `spawn_subagent_from_input`,
            // so we start clean and let the spawn layer populate it (#691).
            worktree_path: None,
        }
    }

    /// Whether the next spawn would exceed the depth cap.
    #[must_use]
    pub fn would_exceed_depth(&self) -> bool {
        self.spawn_depth + 1 > self.max_spawn_depth
    }
}

/// A running sub-agent instance.
pub struct SubAgent {
    pub id: String,
    pub session_name: String,
    pub fork_context: bool,
    pub agent_type: SubAgentType,
    pub prompt: String,
    pub assignment: SubAgentAssignment,
    pub model: String,
    pub nickname: Option<String>,
    pub status: SubAgentStatus,
    pub result: Option<String>,
    pub steps_taken: u32,
    pub checkpoint: Option<SubAgentCheckpoint>,
    pub needs_input: Option<SubAgentNeedsInput>,
    pub started_at: Instant,
    pub last_activity_at: Instant,
    /// `None` = full registry inheritance, with approval-gated tools still
    /// blocked unless the parent runtime is auto-approved.
    /// `Some(list)` = explicit narrow allowlist (Custom agents, legacy).
    pub allowed_tools: Option<Vec<String>>,
    /// Stable id of the manager that spawned this agent (#405). Compared
    /// against the manager's `current_session_boot_id` to classify the
    /// agent as in-session vs prior-session at list time.
    pub session_boot_id: String,
    pub workspace: PathBuf,
    /// Isolated git worktree path created for this agent (only when spawned
    /// with `worktree: true`). `None` for non-worktree spawns. Used by the
    /// manager to `git worktree remove` the checkout on teardown (#691).
    pub worktree_path: Option<PathBuf>,
    input_tx: Option<mpsc::UnboundedSender<SubAgentInput>>,
    task_handle: Option<JoinHandle<()>>,
}

impl SubAgent {
    /// Create a new sub-agent. The `id` is generated by the caller so that
    /// deterministic whale-naming can hash the ID before construction.
    #[allow(clippy::too_many_arguments)]
    fn new(
        id: String,
        agent_type: SubAgentType,
        prompt: String,
        assignment: SubAgentAssignment,
        model: String,
        nickname: Option<String>,
        allowed_tools: Option<Vec<String>>,
        input_tx: mpsc::UnboundedSender<SubAgentInput>,
        workspace: PathBuf,
        session_boot_id: String,
        worktree_path: Option<PathBuf>,
    ) -> Self {
        let session_name = id.clone();

        let started_at = Instant::now();
        Self {
            id,
            session_name,
            fork_context: false,
            agent_type,
            prompt,
            assignment,
            model,
            nickname,
            status: SubAgentStatus::Running,
            result: None,
            steps_taken: 0,
            checkpoint: None,
            needs_input: None,
            started_at,
            last_activity_at: started_at,
            allowed_tools,
            session_boot_id,
            workspace,
            worktree_path,
            input_tx: Some(input_tx),
            task_handle: None,
        }
    }

    /// Get a snapshot of the current state.
    #[must_use]
    pub fn snapshot(&self) -> SubAgentResult {
        SubAgentResult {
            name: self.session_name.clone(),
            agent_id: self.id.clone(),
            context_mode: if self.fork_context { "forked" } else { "fresh" }.to_string(),
            fork_context: self.fork_context,
            workspace: Some(self.workspace.clone()),
            git_branch: current_git_branch(&self.workspace),
            agent_type: self.agent_type.clone(),
            assignment: self.assignment.clone(),
            model: self.model.clone(),
            nickname: self.nickname.clone(),
            status: self.status.clone(),
            worker_status: None,
            parent_run_id: None,
            spawn_depth: 0,
            result: self.result.clone(),
            steps_taken: self.steps_taken,
            checkpoint: self.checkpoint.clone(),
            needs_input: self.needs_input.clone(),
            duration_ms: u64::try_from(self.started_at.elapsed().as_millis()).unwrap_or(u64::MAX),
            // Snapshots from the agent itself don't know the manager's
            // current boot id, so default to false. The manager fills
            // this in when it produces a snapshot via its own
            // `snapshot_for_listing` helper (#405).
            from_prior_session: false,
        }
    }
}

/// Manager for active sub-agents.
pub struct SubAgentManager {
    agents: HashMap<String, SubAgent>,
    worker_records: HashMap<String, AgentWorkerRecord>,
    worker_event_seq: u64,
    workspace: PathBuf,
    state_path: Option<PathBuf>,
    max_steps: u32,
    max_agents: usize,
    max_admitted_agents: usize,
    default_token_budget: Option<u64>,
    running_heartbeat_timeout: Duration,
    /// Stable id assigned at manager construction (#405). Stamped on
    /// every agent the manager spawns; agents loaded from the
    /// persisted state file carry whatever id the prior session
    /// stamped (or empty for pre-#405 records). The manager classifies
    /// agents whose `session_boot_id` doesn't match this value as
    /// "from prior session" so listings can hide them by default.
    current_session_boot_id: String,
    /// Launch gate for direct (depth-1) sub-agent launches (#3095). Each
    /// permit is one actively executing direct child; further direct
    /// children spawn immediately but queue for a permit before starting,
    /// publishing a visible "queued" reason instead of bursting. Deeper
    /// descendants bypass the gate so a permit-holding parent waiting on
    /// its own children cannot deadlock the tree.
    launch_gate: Arc<Semaphore>,
    /// #freeze: hot-path persist debounce bookkeeping (see
    /// `SUBAGENT_PERSIST_DEBOUNCE`). `last_persist_at` is the last time any
    /// state persist ran; `persist_pending` records that a hot-path write was
    /// coalesced away so a later flush (terminal write or shutdown) can
    /// capture the most recent checkpoint.
    last_persist_at: Option<Instant>,
    persist_pending: bool,
    /// Shared agent bus for inter-agent communication.
    bus: Arc<AgentBus>,
    /// Shared task-claim manager so sub-agents can coordinate exclusive
    /// ownership of work items (#699). Sub-agents spawned by the same manager
    /// share this instance.
    task_claims: SharedTaskClaimManager,
}

/// Thread-safe wrapper for `SubAgentManager`.
pub type SharedSubAgentManager = Arc<RwLock<SubAgentManager>>;

pub fn load_persisted_agent_worker_records(workspace: &Path) -> Result<Vec<AgentWorkerRecord>> {
    let mut manager = SubAgentManager::new(workspace.to_path_buf(), 1)
        .with_state_path(default_state_path(workspace)?);
    manager.load_state()?;
    Ok(manager.list_worker_records())
}

/// Model-facing session projection returned by the v0.8.33 sub-agent API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentSessionProjection {
    pub name: String,
    pub agent_id: String,
    #[serde(default)]
    pub run_id: String,
    pub status: String,
    pub terminal: bool,
    pub context_mode: String,
    pub fork_context: bool,
    pub prefix_cache: SubAgentPrefixCacheProjection,
    pub transcript_handle: VarHandle,
    #[serde(default = "default_agent_run_follow_up")]
    pub follow_up: AgentRunFollowUpTarget,
    #[serde(default = "default_agent_run_takeover")]
    pub takeover: AgentRunTakeoverTarget,
    #[serde(default)]
    pub artifacts: Vec<AgentRunArtifactRef>,
    #[serde(default = "default_agent_run_usage")]
    pub usage: AgentRunUsage,
    #[serde(default = "default_agent_run_verification")]
    pub verification: AgentRunVerificationSummary,
    pub snapshot: SubAgentResult,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkpoint: Option<SubAgentCheckpoint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub needs_input: Option<SubAgentNeedsInput>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub continuable: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub needs_continuation: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub timed_out: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub timed_out_with_checkpoint: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_record: Option<AgentWorkerRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentPrefixCacheProjection {
    pub mode: String,
    pub parent_prefix: String,
    pub deepseek_prefix_cache_reuse: String,
}

fn subagent_prefix_cache_projection(snapshot: &SubAgentResult) -> SubAgentPrefixCacheProjection {
    if snapshot.fork_context {
        SubAgentPrefixCacheProjection {
            mode: "forked".to_string(),
            parent_prefix: "preserved_byte_identical_when_available".to_string(),
            deepseek_prefix_cache_reuse: "optimized_for_existing_parent_prefill".to_string(),
        }
    } else {
        SubAgentPrefixCacheProjection {
            mode: "fresh".to_string(),
            parent_prefix: "not_inherited".to_string(),
            deepseek_prefix_cache_reuse: "independent_child_prefill".to_string(),
        }
    }
}

fn subagent_checkpoint_is_continuable(snapshot: &SubAgentResult) -> bool {
    matches!(snapshot.status, SubAgentStatus::Interrupted(_))
        && snapshot
            .checkpoint
            .as_ref()
            .is_some_and(|checkpoint| checkpoint.continuable && !checkpoint.messages.is_empty())
}

async fn subagent_session_projection(
    snapshot: SubAgentResult,
    timed_out: bool,
    context: &ToolContext,
    worker_record: Option<AgentWorkerRecord>,
) -> SubAgentSessionProjection {
    let transcript_session_id = format!("agent:{}", snapshot.agent_id);
    let continuable = subagent_checkpoint_is_continuable(&snapshot);
    let transcript_payload = json!({
        "kind": "subagent_session_snapshot",
        "agent_id": snapshot.agent_id.clone(),
        "name": snapshot.name.clone(),
        "status": subagent_status_name(&snapshot.status),
        "context_mode": snapshot.context_mode.clone(),
        "fork_context": snapshot.fork_context,
        "result": snapshot.result.clone(),
        "steps_taken": snapshot.steps_taken,
        "duration_ms": snapshot.duration_ms,
        "assignment": snapshot.assignment.clone(),
        "checkpoint": snapshot.checkpoint.clone(),
        "needs_input": snapshot.needs_input.clone(),
        "needs_continuation": continuable,
        "timed_out_with_checkpoint": timed_out && continuable,
        "snapshot": snapshot.clone(),
    });
    let transcript_handle = {
        let mut store = context.runtime.handle_store.lock().await;
        let full_transcript_lookup = VarHandle {
            kind: "var_handle".to_string(),
            session_id: transcript_session_id.clone(),
            name: "full_transcript".to_string(),
            type_name: String::new(),
            length: 0,
            repr_preview: String::new(),
            sha256: String::new(),
        };
        if snapshot.status != SubAgentStatus::Running
            && let Some(record) = store.get(&full_transcript_lookup)
        {
            record.handle.clone()
        } else {
            store.insert_json(transcript_session_id, "transcript", transcript_payload)
        }
    };
    let run_id = worker_record
        .as_ref()
        .map(|record| agent_worker_run_id(&record.spec))
        .unwrap_or_else(|| snapshot.agent_id.clone());
    let follow_up = worker_record
        .as_ref()
        .map(|record| record.follow_up.clone())
        .unwrap_or_else(|| AgentRunFollowUpTarget {
            tool: default_agent_inspect_tool(),
            agent_id: snapshot.agent_id.clone(),
            session_name: Some(snapshot.name.clone()),
            accepted_statuses: vec!["running".to_string(), "interrupted_continuable".to_string()],
            latest_delivery: None,
        });
    let takeover = worker_record
        .as_ref()
        .map(|record| record.takeover.clone())
        .unwrap_or_else(|| AgentRunTakeoverTarget {
            kind: default_subagent_takeover_kind(),
            supported: true,
            agent_id: snapshot.agent_id.clone(),
            session_name: Some(snapshot.name.clone()),
            instructions: format!(
                "Inspect agent '{}' through the returned transcript_handle with handle_read; open a replacement with agent if the lane no longer fits.",
                snapshot.agent_id
            ),
            unsupported_reason: None,
        });
    let artifacts = worker_record
        .as_ref()
        .map(|record| record.artifacts.clone())
        .unwrap_or_else(|| default_subagent_artifacts(&run_id));
    let usage = worker_record
        .as_ref()
        .map(|record| record.usage.clone())
        .unwrap_or_else(default_agent_run_usage);
    let verification = worker_record
        .as_ref()
        .map(|record| record.verification.clone())
        .unwrap_or_else(default_agent_run_verification);
    // Status must stay coherent with the continuation flags below. An
    // Interrupted snapshot that carries a continuable checkpoint
    // (`continuable`/`needs_continuation` true, `terminal` true) means the
    // worker is parked waiting for the parent to act, so it must project as
    // `waiting_for_user` rather than a bare `interrupted`. When a worker
    // record exists its status was already derived via
    // `worker_status_from_subagent_result`; mirror that derivation when there
    // is no record so both paths agree on the "needs parent action" signal.
    let status = worker_record
        .as_ref()
        .map(|record| agent_worker_status_name(record.status.clone()))
        .unwrap_or_else(|| agent_worker_status_name(worker_status_from_subagent_result(&snapshot)))
        .to_string();

    SubAgentSessionProjection {
        name: snapshot.name.clone(),
        agent_id: snapshot.agent_id.clone(),
        run_id,
        status,
        terminal: snapshot.status != SubAgentStatus::Running,
        context_mode: snapshot.context_mode.clone(),
        fork_context: snapshot.fork_context,
        prefix_cache: subagent_prefix_cache_projection(&snapshot),
        transcript_handle,
        follow_up,
        takeover,
        artifacts,
        usage,
        verification,
        checkpoint: snapshot.checkpoint.clone(),
        needs_input: snapshot.needs_input.clone(),
        continuable: subagent_checkpoint_is_continuable(&snapshot),
        needs_continuation: continuable,
        snapshot,
        timed_out,
        timed_out_with_checkpoint: timed_out && continuable,
        worker_record,
    }
}

fn default_state_path(workspace: &Path) -> Result<PathBuf> {
    let workspace = normalize_subagent_workspace(workspace);
    // Project-local state path under .mimofan
    let primary = checked_subagent_state_path(
        &workspace,
        &Path::new(".mimofan")
            .join("state")
            .join(SUBAGENT_STATE_FILE),
    )?;
    if primary.exists() {
        return Ok(primary);
    }
    checked_subagent_state_path(
        &workspace,
        &Path::new(".mimofan")
            .join("state")
            .join(SUBAGENT_STATE_FILE),
    )
}

fn checked_subagent_state_path(workspace: &Path, path: &Path) -> Result<PathBuf> {
    let workspace = normalize_subagent_workspace(workspace);
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        workspace.join(path)
    };
    let file_name = absolute
        .file_name()
        .ok_or_else(|| anyhow!("sub-agent state path must include a file name"))?;
    let parent = absolute
        .parent()
        .ok_or_else(|| anyhow!("sub-agent state path must include a parent directory"))?;
    let parent = match parent.canonicalize() {
        Ok(parent) => parent,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => normalize_path_components(parent),
        Err(err) => return Err(err.into()),
    };
    let state_path = parent.join(file_name);
    if !state_path.starts_with(&workspace) {
        return Err(anyhow!(
            "sub-agent state path must stay within workspace: {}",
            state_path.display()
        ));
    }
    reject_workspace_relative_symlinks(&workspace, &state_path)?;
    Ok(state_path)
}

fn normalize_subagent_workspace(workspace: &Path) -> PathBuf {
    if let Ok(canonical) = workspace.canonicalize() {
        return canonical;
    }
    let absolute = if workspace.is_absolute() {
        workspace.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(workspace)
    };
    normalize_path_components(&absolute)
}

fn reject_workspace_relative_symlinks(workspace: &Path, path: &Path) -> Result<()> {
    let relative = path.strip_prefix(workspace).map_err(|_| {
        anyhow!(
            "sub-agent state path must stay within workspace: {}",
            path.display()
        )
    })?;
    let mut current = workspace.to_path_buf();
    for component in relative.components() {
        current.push(component.as_os_str());
        let Ok(metadata) = fs::symlink_metadata(&current) else {
            continue;
        };
        if metadata.file_type().is_symlink() {
            return Err(anyhow!(
                "sub-agent state path must not traverse symlinks: {}",
                current.display()
            ));
        }
    }
    Ok(())
}

fn read_subagent_state_file(workspace: &Path, path: &Path) -> Result<String> {
    let workspace = normalize_subagent_workspace(workspace);
    reject_workspace_relative_symlinks(&workspace, path)?;
    let metadata = fs::symlink_metadata(path)?;
    let file_type = metadata.file_type();
    if file_type.is_symlink() || !file_type.is_file() {
        return Err(anyhow!(
            "sub-agent state path must be a regular file: {}",
            path.display()
        ));
    }

    let mut file = open_subagent_state_file(path)?;
    let mut raw = String::new();
    file.read_to_string(&mut raw)?;
    Ok(raw)
}

#[cfg(unix)]
fn open_subagent_state_file(path: &Path) -> Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;

    fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(Into::into)
}

#[cfg(not(unix))]
fn open_subagent_state_file(path: &Path) -> Result<fs::File> {
    fs::File::open(path).map_err(Into::into)
}

fn epoch_millis_now() -> u64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => u64::try_from(duration.as_millis()).unwrap_or(u64::MAX),
        Err(_) => 0,
    }
}

fn instant_from_duration(duration: Duration) -> Instant {
    Instant::now()
        .checked_sub(duration)
        .unwrap_or_else(Instant::now)
}

fn write_json_atomic<T: Serialize>(workspace: &Path, path: &Path, value: &T) -> Result<()> {
    let workspace = normalize_subagent_workspace(workspace);
    reject_workspace_relative_symlinks(&workspace, path)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let payload = serde_json::to_string_pretty(value)?;
    let tmp_path = path.with_extension(format!("{}.tmp", std::process::id()));
    reject_workspace_relative_symlinks(&workspace, &tmp_path)?;
    fs::write(&tmp_path, payload)?;
    fs::rename(tmp_path, path)?;
    Ok(())
}

/// Create a shared sub-agent manager with a configurable limit.
#[cfg(test)]
#[must_use]
pub fn new_shared_subagent_manager(workspace: PathBuf, max_agents: usize) -> SharedSubAgentManager {
    new_shared_subagent_manager_with_timeout(
        workspace,
        max_agents,
        max_agents,
        Duration::from_secs(crate::config::DEFAULT_SUBAGENT_HEARTBEAT_TIMEOUT_SECS),
        max_agents,
        None,
    )
}

/// Create a shared sub-agent manager with configurable concurrency and stale
/// running-agent heartbeat timeout.
#[must_use]
pub fn new_shared_subagent_manager_with_timeout(
    workspace: PathBuf,
    max_agents: usize,
    max_admitted_agents: usize,
    running_heartbeat_timeout: Duration,
    launch_concurrency: usize,
    default_token_budget: Option<u64>,
) -> SharedSubAgentManager {
    let max_agents = max_agents.clamp(1, MAX_SUBAGENTS);
    let state_path = match default_state_path(&workspace) {
        Ok(path) => Some(path),
        Err(err) => {
            tracing::warn!(target: "subagent", ?err, "failed to resolve sub-agent state path");
            None
        }
    };
    let mut manager = SubAgentManager::new(workspace, max_agents)
        .with_admission_limit(max_admitted_agents)
        .with_running_heartbeat_timeout(running_heartbeat_timeout)
        .with_launch_concurrency(launch_concurrency)
        .with_default_token_budget(default_token_budget);
    if let Some(state_path) = state_path {
        manager = manager.with_state_path(state_path);
    }
    if let Err(err) = manager.load_state() {
        // Routed through tracing instead of stderr — see comment in
        // `persist_state_best_effort` above.
        tracing::warn!(target: "subagent", ?err, "failed to load sub-agent state");
    }
    Arc::new(RwLock::new(manager))
}

// === Sub-agent Execution ===

/// Build the system prompt for a sub-agent.
///
/// Starts with the per-type prompt (`SubAgentType::system_prompt`) and
/// appends a one-line role overlay when `assignment.role` is set. The
/// full role library — TOML overlays from `~/.mimofan/roles/`, the
/// `/roles` slash command, model overrides per role — lands in 0.6.7.
/// For 0.6.6 we just don't drop the role on the floor: the model sees
/// "You are operating in the role of `{name}`." as a final line so its
/// behavior reflects the user's choice.
///
/// If a custom agent definition is provided, its prompt is used instead
/// of the built-in type prompt.
fn build_subagent_system_prompt(
    agent_type: &SubAgentType,
    assignment: &SubAgentAssignment,
    custom_agent_def: Option<&custom_agents::CustomAgentDef>,
) -> String {
    // Use custom agent's prompt if available, otherwise use built-in type prompt
    let base = if let Some(def) = custom_agent_def {
        def.prompt.clone()
    } else {
        agent_type.system_prompt()
    };

    let mut prompt = match assignment.role.as_deref() {
        Some(role) if !role.trim().is_empty() => {
            format!(
                "{base}\n\nYou are operating in the role of `{}`.",
                role.trim()
            )
        }
        _ => base,
    };
    // Sub-agents are background workers: the orchestrating agent is their only
    // caller. They never talk to the end user.
    prompt.push_str(
        "\n\nYou are a background sub-agent: every instruction comes from the orchestrating agent, not a human. Never address the end user or ask them questions — do the assigned work and report results back to the orchestrator.",
    );
    prompt
}

fn subagent_request_system_prompt(
    subagent_system_prompt: &str,
    fork_context: Option<&SubAgentForkContext>,
) -> SystemPrompt {
    fork_context
        .and_then(|context| context.system.clone())
        .unwrap_or_else(|| SystemPrompt::Text(subagent_system_prompt.to_string()))
}

fn build_initial_subagent_messages(
    prompt: &str,
    assignment: &SubAgentAssignment,
    agent_type: &SubAgentType,
    fork_context: Option<&SubAgentForkContext>,
    custom_agent_def: Option<&custom_agents::CustomAgentDef>,
) -> Vec<Message> {
    let mut messages = fork_context
        .map(|context| context.messages.clone())
        .unwrap_or_default();

    if let Some(context) = fork_context {
        if let Some(state) = context
            .structured_state_block
            .as_deref()
            .map(str::trim)
            .filter(|state| !state.is_empty())
        {
            messages.push(system_text_message(format!(
                "<mimo:fork_state>\n{state}\n</mimo:fork_state>"
            )));
        }

        messages.push(system_text_message(format!(
            "<mimo:subagent_context>\n{}\n</mimo:subagent_context>",
            build_subagent_system_prompt(agent_type, assignment, custom_agent_def)
        )));
    }

    messages.push(Message {
        role: "user".to_string(),
        content: vec![ContentBlock::Text {
            text: build_assignment_prompt(prompt, assignment, agent_type),
            cache_control: None,
        }],
    });

    messages
}

fn system_text_message(text: String) -> Message {
    Message {
        role: "system".to_string(),
        content: vec![ContentBlock::Text {
            text,
            cache_control: None,
        }],
    }
}

struct SubAgentTask {
    manager_handle: SharedSubAgentManager,
    runtime: SubAgentRuntime,
    agent_id: String,
    agent_type: SubAgentType,
    prompt: String,
    assignment: SubAgentAssignment,
    /// `None` = full registry inheritance. `Some(list)` = explicit narrow.
    /// Approval-gated tools still require an auto-approved parent runtime.
    allowed_tools: Option<Vec<String>>,
    fork_context: bool,
    started_at: Instant,
    max_steps: u32,
    /// Per-worker token cap sourced from the spawn request's `token_budget`
    /// (the explicit `max_tokens`/`tokenBudget` override). `None` means no
    /// per-worker limit; the worker still obeys the scope admission gate.
    /// When set, the worker stops with `BudgetExhausted` once its accumulated
    /// model tokens exceed this value. Independent of the scope budget (#3319).
    token_budget: Option<u64>,
    input_rx: mpsc::UnboundedReceiver<SubAgentInput>,
    /// Custom agent definition loaded from Markdown file (if applicable).
    custom_agent_def: Option<custom_agents::CustomAgentDef>,
    /// Interactive launch gate (#3095). `Some` only for direct (depth-1)
    /// children: the task acquires a permit before its first model step and
    /// holds it until completion, so a fanout burst beyond the limit queues
    /// with a visible reason instead of executing all at once.
    launch_gate: Option<Arc<Semaphore>>,
}

#[allow(clippy::too_many_lines)]
async fn run_subagent_task(task: SubAgentTask) {
    // Interactive launch gate (#3095): direct children acquire a permit
    // before their first model step so a fanout burst beyond the limit
    // queues visibly instead of executing all at once. The permit is held
    // for the lifetime of the task. Cancellation while queued is handled by
    // `run_subagent`'s own first-step cancel check.
    let mut _launch_permit = None;
    if let Some(gate) = task.launch_gate.as_ref() {
        match Arc::clone(gate).try_acquire_owned() {
            Ok(permit) => _launch_permit = Some(permit),
            Err(tokio::sync::TryAcquireError::NoPermits) => {
                _launch_permit = acquire_queued_launch_permit(&task, Arc::clone(gate)).await;
            }
            Err(tokio::sync::TryAcquireError::Closed) => {
                crate::logging::warn(format!(
                    "sub-agent launch gate closed for {}; proceeding without backpressure",
                    task.agent_id
                ));
            }
        }
    }

    let result = run_subagent(
        &task.runtime,
        task.agent_id.clone(),
        task.agent_type,
        task.prompt,
        task.assignment,
        task.allowed_tools,
        task.fork_context,
        task.started_at,
        task.max_steps,
        task.token_budget,
        task.input_rx,
        task.custom_agent_def,
    )
    .await;

    // Emit BOTH a human-friendly summary (rendered in the parent's
    // sidebar / cell) AND a structured sentinel the model can recognize
    // on its next turn. Format: human summary on the first line,
    // sentinel on the second. The sentinel uses an opaque tag
    // (`mimo:subagent.done`) to avoid collision with normal user
    // text.
    let model_id = task.runtime.model.clone();
    let (summary, sentinel) = match &result {
        Ok(res) => {
            // Issue #2652: the child's free-text result is its self-report, not
            // verified evidence. Stamp it with a provenance marker: a soft
            // "re-verify" note when short, or a head+tail truncation (reusing
            // the tool-output vocabulary) when it exceeds the wire budget. The
            // resulting `truncated` flag is carried in the sentinel so the
            // parent model can branch on `summary_kind`.
            let raw = summarize_subagent_result(res);
            let (summary, truncated) = stamp_subagent_summary(&raw);
            let sentinel = subagent_done_sentinel(&task.agent_id, res, truncated);
            (summary, sentinel)
        }
        Err(err) => {
            let annotated = annotate_child_model_error(&err.to_string(), &model_id);
            (
                format!("Failed: {annotated}"),
                subagent_failed_sentinel(&task.agent_id, &annotated),
            )
        }
    };

    if let Some(mb) = task.runtime.mailbox.as_ref() {
        let envelope = match &result {
            Ok(_) => MailboxMessage::Completed {
                agent_id: task.agent_id.clone(),
                summary: summary.clone(),
            },
            Err(err) => MailboxMessage::Failed {
                agent_id: task.agent_id.clone(),
                error: annotate_child_model_error(&err.to_string(), &model_id),
            },
        };
        let _ = mb.send(envelope);
    }

    let payload = format!("{summary}\n{sentinel}");
    let agent_id = task.agent_id.clone();

    // Wake the engine's parent turn loop if this is one of its direct
    // children (issue #756). Issue #1961 also requires emit to happen
    // before marking the manager terminal state so the parent can observe the
    // completion while its "running children" gate is still open. If we
    // update first, the parent can finalize before the completion arrives.
    emit_parent_completion(&task.runtime, &agent_id, &payload);

    let mut manager = task.manager_handle.write().await;
    match &result {
        Ok(res) => manager.update_from_result(&agent_id, res.clone()),
        Err(err) => {
            manager.update_failed(
                &agent_id,
                annotate_child_model_error(&err.to_string(), &model_id),
            );
        }
    }

    if let Some(event_tx) = task.runtime.event_tx {
        let _ = event_tx.try_send(Event::AgentComplete {
            id: agent_id.clone(),
            result: payload,
        });
    }
}

async fn acquire_queued_launch_permit(
    task: &SubAgentTask,
    gate: Arc<Semaphore>,
) -> Option<tokio::sync::OwnedSemaphorePermit> {
    record_queued_launch_progress(task).await;
    tokio::select! {
        biased;
        () = task.runtime.cancel_token.cancelled() => {
            record_agent_progress(
                &task.runtime,
                &task.agent_id,
                "cancelled while queued for a sub-agent launch slot".to_string(),
            );
            None
        }
        permit = Arc::clone(&gate).acquire_owned() => {
            permit.ok()
        }
    }
}

async fn record_queued_launch_progress(task: &SubAgentTask) {
    {
        let mut manager = task.runtime.manager.write().await;
        manager.touch(&task.agent_id);
        manager.record_worker_event(
            &task.agent_id,
            AgentWorkerStatus::Queued,
            Some(SUBAGENT_QUEUED_LAUNCH_REASON.to_string()),
            None,
            None,
        );
    }
    emit_agent_progress(
        task.runtime.event_tx.as_ref(),
        &task.agent_id,
        SUBAGENT_QUEUED_LAUNCH_REASON.to_string(),
        task.runtime.parent_agent_id.clone(),
        task.runtime.spawn_depth,
    );
    if let Some(mailbox) = task.runtime.mailbox.as_ref() {
        let _ = mailbox.send(MailboxMessage::progress(
            &task.agent_id,
            SUBAGENT_QUEUED_LAUNCH_REASON,
        ));
    }
}

/// Notify this runtime's immediate parent that the child finished (issue
/// #756). Root-spawned children send to the engine turn loop. Nested children
/// send to the parent sub-agent's local inbox, which is swapped into the
/// runtime used by that parent's `agent` tool. Returns `true` if a send was
/// attempted, `false` if this is the engine itself or no channel is wired.
/// Skips silently when the channel sender has no receiver — the receiver may
/// have ended because the parent turn/agent already completed.
pub(crate) fn emit_parent_completion(
    runtime: &SubAgentRuntime,
    agent_id: &str,
    payload: &str,
) -> bool {
    if runtime.spawn_depth == 0 {
        return false;
    }
    let Some(tx) = runtime.parent_completion_tx.as_ref() else {
        return false;
    };
    let _ = tx.send(SubAgentCompletion {
        agent_id: agent_id.to_string(),
        payload: payload.to_string(),
    });
    true
}

pub(crate) fn subagent_completion_from_result(result: &SubAgentResult) -> SubAgentCompletion {
    let raw = summarize_subagent_result(result);
    let (summary, truncated) = stamp_subagent_summary(&raw);
    let sentinel = match &result.status {
        SubAgentStatus::Failed(error) => subagent_failed_sentinel(&result.agent_id, error),
        _ => subagent_done_sentinel(&result.agent_id, result, truncated),
    };
    SubAgentCompletion {
        agent_id: result.agent_id.clone(),
        payload: format!("{summary}\n{sentinel}"),
    }
}

/// Build a `<mimo:subagent.done>` JSON sentinel for a successful child.
/// Intended to surface in the parent's transcript so the model recognizes
/// child completion.
///
/// Keep this payload deliberately lean. The human summary is emitted on the
/// line immediately before the sentinel; duplicating it here bloats the next
/// parent request's cache-miss tail. Wall-clock duration is useful UI
/// telemetry, but it is volatile and not useful for model coordination.
///
/// `truncated` reflects whether the previous-line summary was length-gated by
/// [`stamp_subagent_summary`] (issue #2652); it surfaces as `summary_kind` so
/// the parent model can tell a complete self-report from a clipped one and
/// verify material claims accordingly.
fn subagent_done_sentinel(agent_id: &str, res: &SubAgentResult, truncated: bool) -> String {
    let mut payload = json!({
        "agent_id": agent_id,
        // Mimofan name — a stable, human-friendly handle the orchestrator can use
        // to refer to this child in its own reasoning/output.
        "name": res.nickname,
        "agent_type": res.agent_type.as_str(),
        "status": subagent_status_name(&res.status),
        "summary_location": "previous_line",
        // issue #2652: lets the parent branch on whether the previous-line
        // summary is the full child report or a head+tail excerpt.
        "summary_kind": if truncated { "truncated" } else { "complete" },
    });
    if let Some(needs_input) = res.needs_input.clone() {
        payload["needs_input"] = json!(needs_input);
    }
    format!("<mimo:subagent.done>{payload}</mimo:subagent.done>")
}

/// Build a `<mimo:subagent.done>` sentinel for a failed child.
///
/// Kept lean: the (annotated) error is on the previous line (`error_location`)
/// so the sentinel only signals completion state rather than re-embedding the
/// error text.
fn subagent_failed_sentinel(agent_id: &str, _err: &str) -> String {
    let payload = json!({
        "agent_id": agent_id,
        "status": "failed",
        "error_location": "previous_line",
    });
    format!("<mimo:subagent.done>{payload}</mimo:subagent.done>")
}

fn response_was_truncated(response: &MessageResponse) -> bool {
    response.stop_reason.as_deref() == Some("length")
}

fn truncated_response_tool_results(tool_uses: &[(String, String, Value)]) -> Vec<ContentBlock> {
    tool_uses
        .iter()
        .map(|(tool_id, tool_name, _)| ContentBlock::ToolResult {
            tool_use_id: tool_id.clone(),
            content: format!(
                "Error: the model response was truncated by max_tokens before the tool call arguments for '{tool_name}' could be fully generated. Split large content into smaller writes and retry."
            ),
            is_error: Some(true),
            content_blocks: None,
        })
        .collect()
}

fn truncated_response_text_retry_message() -> Vec<ContentBlock> {
    vec![ContentBlock::Text {
        text: "Error: the model response was truncated by max_tokens. No complete tool call was available, so the partial response was not accepted as the sub-agent result. Retry with a shorter response or split the work into smaller steps.".to_string(),
        cache_control: None,
    }]
}

fn record_truncated_subagent_response(consecutive: &mut u32) -> Result<()> {
    *consecutive = consecutive.saturating_add(1);
    if *consecutive > MAX_CONSECUTIVE_TRUNCATED_SUBAGENT_RESPONSES {
        return Err(anyhow!(
            "Sub-agent response was truncated by max_tokens {count} consecutive times; stopping to avoid an unbounded retry loop.",
            count = *consecutive
        ));
    }
    Ok(())
}

fn reset_truncated_subagent_responses(consecutive: &mut u32) {
    *consecutive = 0;
}

#[allow(clippy::too_many_arguments)]
async fn insert_subagent_full_transcript_handle(
    runtime: &SubAgentRuntime,
    agent_id: &str,
    agent_type: &SubAgentType,
    assignment: &SubAgentAssignment,
    status: &SubAgentStatus,
    result: Option<&String>,
    checkpoint: Option<&SubAgentCheckpoint>,
    messages: &[Message],
    steps_taken: u32,
    duration_ms: u64,
    fork_context: bool,
) -> VarHandle {
    let payload = json!({
        "kind": "subagent_full_transcript",
        "agent_id": agent_id,
        "agent_type": agent_type.as_str(),
        "status": subagent_status_name(status),
        "context_mode": if fork_context { "forked" } else { "fresh" },
        "fork_context": fork_context,
        "result": result,
        "steps_taken": steps_taken,
        "duration_ms": duration_ms,
        "assignment": assignment,
        "checkpoint": checkpoint,
        "messages": messages,
    });
    let mut store = runtime.context.runtime.handle_store.lock().await;
    store.insert_json(format!("agent:{agent_id}"), "full_transcript", payload)
}

fn build_subagent_checkpoint(
    agent_id: &str,
    reason: impl Into<String>,
    messages: &[Message],
    steps_taken: u32,
    continuable: bool,
) -> SubAgentCheckpoint {
    let created_at_ms = epoch_millis_now();
    let checkpoint_id = format!("{agent_id}:step:{steps_taken}:ts:{created_at_ms}");
    SubAgentCheckpoint {
        checkpoint_id: checkpoint_id.clone(),
        agent_id: agent_id.to_string(),
        continuation_handle: format!("agent:{agent_id}:checkpoint:{checkpoint_id}"),
        reason: reason.into(),
        continuable,
        steps_taken,
        message_count: messages.len(),
        created_at_ms,
        messages: messages.to_vec(),
    }
}

async fn checkpoint_subagent_progress(
    runtime: &SubAgentRuntime,
    agent_id: &str,
    reason: impl Into<String>,
    messages: &[Message],
    steps_taken: u32,
    continuable: bool,
) -> SubAgentCheckpoint {
    let checkpoint =
        build_subagent_checkpoint(agent_id, reason, messages, steps_taken, continuable);
    let mut manager = runtime.manager.write().await;
    manager.update_checkpoint(agent_id, checkpoint.clone());
    checkpoint
}

fn needs_input_for_interrupted_checkpoint(
    reason: &str,
    checkpoint: &SubAgentCheckpoint,
) -> SubAgentNeedsInput {
    SubAgentNeedsInput {
        question: format!(
            "Sub-agent interrupted before completion ({reason}). Re-dispatch this worker or provide explicit follow-up using checkpoint {}.",
            checkpoint.continuation_handle
        ),
    }
}

#[derive(Debug)]
enum SubAgentApiRequestFailure {
    Fatal(anyhow::Error),
    Interrupted {
        reason: String,
        checkpoint_reason: &'static str,
    },
}

fn subagent_transient_provider_retry_delay(retry_number: u32) -> Duration {
    let multiplier = 1u32
        .checked_shl(retry_number.saturating_sub(1))
        .unwrap_or(4);
    SUBAGENT_TRANSIENT_PROVIDER_INITIAL_BACKOFF.saturating_mul(multiplier.min(4))
}

fn is_transient_subagent_provider_error(error: &anyhow::Error) -> bool {
    let message = format!("{error:#}").to_ascii_lowercase();
    [
        "did not receive response headers",
        "response headers",
        "stream request",
        "request timed out",
        "operation timed out",
        "deadline has elapsed",
        "connection reset",
        "connection closed",
        "connection aborted",
        "temporarily unavailable",
        "bad gateway",
        "gateway timeout",
        "service unavailable",
        "502",
        "503",
        "504",
    ]
    .iter()
    .any(|needle| message.contains(needle))
}

async fn request_subagent_model_response_with_retries(
    runtime: &SubAgentRuntime,
    agent_id: &str,
    steps: u32,
    max_steps: u32,
    request: MessageRequest,
) -> std::result::Result<MessageResponse, SubAgentApiRequestFailure> {
    let mut transient_failures = 0u32;

    loop {
        match tokio::time::timeout(
            runtime.step_api_timeout,
            runtime.client.create_message(request.clone()),
        )
        .await
        {
            Ok(Ok(response)) => return Ok(response),
            Ok(Err(err)) if is_transient_subagent_provider_error(&err) => {
                if transient_failures >= SUBAGENT_TRANSIENT_PROVIDER_MAX_RETRIES {
                    let attempts = transient_failures.saturating_add(1);
                    return Err(SubAgentApiRequestFailure::Interrupted {
                        reason: format!(
                            "Transient provider failure after {attempts} API attempt(s): {err}; checkpoint preserved for continuation"
                        ),
                        checkpoint_reason: "api_transient_provider_failure",
                    });
                }

                transient_failures = transient_failures.saturating_add(1);
                let delay = subagent_transient_provider_retry_delay(transient_failures);
                record_agent_progress(
                    runtime,
                    agent_id,
                    format!(
                        "{}: transient provider failure; retrying API request {}/{} in {}ms ({err})",
                        format_step_counter(steps, max_steps),
                        transient_failures,
                        SUBAGENT_TRANSIENT_PROVIDER_MAX_RETRIES,
                        delay.as_millis(),
                    ),
                );
                tokio::time::sleep(delay).await;
            }
            Ok(Err(err)) => return Err(SubAgentApiRequestFailure::Fatal(err)),
            Err(_) => {
                return Err(SubAgentApiRequestFailure::Interrupted {
                    reason: format!(
                        "API call timed out after {}ms; checkpoint preserved for continuation",
                        runtime.step_api_timeout.as_millis()
                    ),
                    checkpoint_reason: "api_timeout",
                });
            }
        }
    }
}

fn record_agent_progress(runtime: &SubAgentRuntime, agent_id: &str, message: impl Into<String>) {
    let message = message.into();
    if let Ok(mut manager) = runtime.manager.try_write() {
        manager.touch(agent_id);
        manager.record_worker_progress(agent_id, message.clone());
    }
    emit_agent_progress(
        runtime.event_tx.as_ref(),
        agent_id,
        message,
        runtime.parent_agent_id.clone(),
        runtime.spawn_depth,
    );
}

fn runtime_for_nested_agent_tools(
    runtime: &SubAgentRuntime,
    parent_agent_id: &str,
    fork_context: SubAgentForkContext,
) -> (SubAgentRuntime, mpsc::UnboundedReceiver<SubAgentCompletion>) {
    let (child_completion_tx, child_completion_rx) =
        mpsc::unbounded_channel::<SubAgentCompletion>();
    let runtime_for_tools = runtime
        .clone()
        .with_parent_completion_tx(child_completion_tx)
        .with_fork_context(fork_context);
    let runtime_for_tools = SubAgentRuntime {
        parent_agent_id: Some(parent_agent_id.to_string()),
        ..runtime_for_tools
    };
    (runtime_for_tools, child_completion_rx)
}

fn drain_child_completion_events(
    child_completion_rx: &mut mpsc::UnboundedReceiver<SubAgentCompletion>,
) -> Vec<SubAgentCompletion> {
    let mut completions = Vec::new();
    while let Ok(completion) = child_completion_rx.try_recv() {
        completions.push(completion);
    }
    completions
}

fn child_completion_runtime_message(completions: &[SubAgentCompletion]) -> Message {
    let header = "<mimo:runtime_event kind=\"child_subagent_completion\" visibility=\"internal\">\n\
This is an internal runtime event, not user input. One or more child sub-agents \
you spawned have finished. Treat each child summary as an unverified self-report: \
if you rely on it, cite the child agent_id and the EVIDENCE lines it provided, \
and distinguish that from evidence you personally verified.\n";

    // Enforce MAX_SUBAGENT_INJECTION_CHARS: keep the most-recent events that fit.
    let entries: Vec<String> = completions
        .iter()
        .map(|c| {
            format!(
                "\n--- child sub-agent completion ---\nagent_id: {}\n{}\n",
                c.agent_id, c.payload,
            )
        })
        .collect();

    let closing = "</mimo:runtime_event>";
    let note_overhead = 60; // "[Note: N older completion(s) dropped ...]"
    let budget =
        MAX_SUBAGENT_INJECTION_CHARS.saturating_sub(header.len() + closing.len() + note_overhead);

    let mut text = String::from(header);
    let mut used = 0usize;
    let mut included = 0usize;
    for entry in &entries {
        if used + entry.len() > budget && included > 0 {
            break;
        }
        text.push_str(entry);
        used += entry.len();
        included += 1;
    }
    let dropped = entries.len() - included;
    if dropped > 0 {
        text.push_str(&format!(
            "\n[Note: {dropped} older completion(s) dropped to stay within injection budget.]",
        ));
    }
    text.push_str(closing);

    Message {
        role: "user".to_string(),
        content: vec![ContentBlock::Text {
            text,
            cache_control: None,
        }],
    }
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
struct SubAgentToolRegistry {
    /// `None` → full inheritance (no allowlist filter applied). `Some(list)` →
    /// only the listed tools are visible to the model and callable.
    allowed_tools: Option<Vec<String>>,
    auto_approve: bool,
    /// The role/type of the sub-agent that this registry belongs to. Used to
    /// decide whether `Suggest`-level tools (write/edit/patch) may run inside
    /// the child without the parent runtime being auto-approved (#1828, #1833).
    agent_type: SubAgentType,
    can_spawn_child: bool,
    owner_agent_id: String,
    owner_agent_name: String,
    registry: ToolRegistry,
}

impl SubAgentToolRegistry {
    fn new_with_owner(
        runtime: SubAgentRuntime,
        agent_type: SubAgentType,
        owner_agent_id: String,
        owner_agent_name: String,
        explicit_allowed_tools: Option<Vec<String>>,
        todo_list: SharedTodoList,
        plan_state: SharedPlanState,
    ) -> Self {
        // Build the full agent surface — same as the parent's Agent mode.
        // Children inherit shell, file, patch, search, web, git, diagnostics,
        // review, and RLM, plus per-child fresh todo/plan state. `agent` is
        // retained only when depth budget remains.
        let can_spawn_child = !runtime.would_exceed_depth();
        let context = runtime.context.clone();
        let mut registry = ToolRegistryBuilder::new().with_full_agent_surface(
            Some(runtime.client.clone()),
            runtime.model.clone(),
            runtime.manager.clone(),
            runtime.clone(),
            runtime.allow_shell,
            todo_list,
            plan_state,
        );

        if let Some(pool) = runtime.mcp_pool.as_ref() {
            registry = registry.with_mcp_tools(std::sync::Arc::clone(pool));
        }

        let registry = registry.build(context);

        Self {
            allowed_tools: explicit_allowed_tools,
            auto_approve: runtime.context.auto_approve,
            agent_type,
            can_spawn_child,
            owner_agent_id,
            owner_agent_name,
            registry,
        }
    }

    /// Whether this role is allowed to use `Suggest`-level tools (write_file,
    /// edit_file, apply_patch, ...) without the parent runtime being
    /// auto-approved. Read-only stances (`explore`, `plan`, `review`,
    /// `verifier`) stay blocked so they can't quietly mutate the workspace
    /// while a non-auto parent is delegating bounded investigation.
    /// `Required`-level tools (shell, etc.) still need parent auto-approve
    /// regardless of role (#1828, #1833).
    fn role_can_delegate_writes(agent_type: &SubAgentType) -> bool {
        matches!(agent_type, SubAgentType::Implementer | SubAgentType::Custom)
    }

    /// Whether the role posture permits a given registered tool, independent of
    /// parent auto-approval. Delegates to the pure `role_posture_permits`.
    /// Unregistered names pass through (the allowlist / availability checks
    /// handle those separately).
    fn posture_permits_tool(&self, name: &str) -> bool {
        // Delegation (`agent`) is governed by the depth budget and the
        // allowlist (`can_spawn_child` / `is_tool_allowed`), not the write/shell
        // posture — a read-only role may still fan out child work.
        if name == "agent" {
            return true;
        }
        match self.registry.get(name) {
            Some(spec) => role_posture_permits(&self.agent_type, spec.approval_requirement()),
            None => true,
        }
    }

    /// Whether a given tool name is permitted under this child's filter.
    /// `None` filter = everything permitted.
    fn is_tool_allowed(&self, name: &str) -> bool {
        if name == "agent" && !self.can_spawn_child {
            return false;
        }
        match &self.allowed_tools {
            None => true,
            Some(list) => list.iter().any(|t| t == name),
        }
    }

    fn tools_for_model(&self, agent_type: &SubAgentType) -> Vec<Tool> {
        let _ = agent_type;
        let api_tools = self.registry.to_api_tools();
        let filtered = match &self.allowed_tools {
            None => api_tools,
            Some(list) => api_tools
                .into_iter()
                .filter(|tool| list.contains(&tool.name))
                .collect::<Vec<_>>(),
        };
        filtered
            .into_iter()
            .filter(|tool| tool.name != "agent" || self.can_spawn_child)
            // #3217: hide tools the role posture forbids so the model never
            // even sees write/edit/patch (read-only roles) or shell (no-shell
            // roles). Defense-in-depth with the `execute` guard below.
            .filter(|tool| self.posture_permits_tool(&tool.name))
            .collect()
    }

    fn unavailable_allowed_tools(&self) -> Vec<String> {
        match &self.allowed_tools {
            None => Vec::new(),
            Some(list) => list
                .iter()
                .filter(|name| !self.registry.contains(name))
                .cloned()
                .collect(),
        }
    }

    async fn execute(&self, _agent_id: &str, name: &str, input: Value) -> Result<String> {
        if !self.is_tool_allowed(name) {
            return Err(anyhow!("Tool {name} not allowed for this sub-agent"));
        }
        // #3217: authoritative per-role posture — read-only roles cannot mutate
        // and non-`Full`-shell roles cannot run shell, regardless of whether
        // the parent session is auto-approved. This closes the auto-approve
        // bypass where a read-only child could quietly write or shell out.
        if !self.posture_permits_tool(name) {
            return Err(anyhow!(
                "Tool {name} is not permitted for the read-only `{role}` sub-agent role. Use an `implementer` or `general` role (or a `custom` role with an explicit allowed_tools list) to mutate the workspace or run shell commands.",
                role = self.agent_type.as_str()
            ));
        }
        if !self.auto_approve {
            let Some(spec) = self.registry.get(name) else {
                return Err(anyhow!("Tool {name} is not registered"));
            };
            match spec.approval_requirement() {
                ApprovalRequirement::Auto => {}
                ApprovalRequirement::Suggest => {
                    // Write/edit/patch tools land here. Explicit
                    // write-capable roles (`implementer`, `custom`) may run them
                    // without parent auto-approve so that delegated work
                    // can actually land file changes; the previous
                    // behavior blocked every write under `suggest` mode
                    // even for the role explicitly chartered to write
                    // (#1828, #1833). Read-only roles still bounce so
                    // exploration/review/planning/verifier children
                    // can't mutate the workspace behind the parent's back.
                    if !Self::role_can_delegate_writes(&self.agent_type) {
                        return Err(anyhow!(
                            "Tool {name} requires approval and is not delegated to {role} sub-agents; rerun the parent with auto approval or pick a write-capable role",
                            role = self.agent_type.as_str()
                        ));
                    }
                }
                ApprovalRequirement::Required => {
                    return Err(anyhow!(
                        "Tool {name} requires approval and cannot run inside this sub-agent unless the parent session is auto-approved"
                    ));
                }
            }
        }
        reject_subagent_terminal_takeover(name, &input)?;
        let context = self
            .registry
            .context()
            .clone()
            .with_owner_agent(self.owner_agent_id.clone(), self.owner_agent_name.clone());
        self.registry
            .execute_full_with_context(name, input, Some(&context))
            .await
            .map(|result| result.content)
            .map_err(|e| anyhow!(e))
    }
}

fn reject_subagent_terminal_takeover(name: &str, input: &Value) -> Result<()> {
    let wants_interactive_shell = name == "exec_shell"
        && input
            .get("interactive")
            .and_then(Value::as_bool)
            .unwrap_or(false);
    if wants_interactive_shell {
        return Err(anyhow!(
            "Sub-agents run in the background and cannot use exec_shell with interactive=true \
             because that would take over the parent TUI terminal. Use non-interactive \
             exec_shell, background=true, tty=true, or task_shell_start instead."
        ));
    }
    Ok(())
}

/// Resolve the effective allowed-tools list for a child.
///
/// **v0.6.6 default: full inheritance.** Returning `Ok(None)` means the
/// child sees the same tool surface as the parent's Agent mode — every
/// family including `with_subagent_tools` so it can recurse. The narrowing
/// path (`Ok(Some(list))`) is only used by:
/// - `Custom` agent types (which require an explicit list).
/// - Callers that pass `explicit_tools` (advanced / legacy use).
///
/// `allow_shell = false` no longer narrows the tool LIST — the child's
/// registry simply doesn't register shell tools, which has the same
/// effect without papering over the parent's choice with a deny-list.
fn build_allowed_tools(
    agent_type: &SubAgentType,
    explicit_tools: Option<Vec<String>>,
    _allow_shell: bool,
) -> Result<Option<Vec<String>>> {
    if let Some(tools) = explicit_tools {
        let mut deduped = Vec::new();
        for tool in tools {
            let name = tool.trim();
            if !name.is_empty() && !deduped.iter().any(|existing: &String| existing == name) {
                deduped.push(name.to_string());
            }
        }
        if matches!(agent_type, SubAgentType::Custom) && deduped.is_empty() {
            return Err(anyhow!(
                "Custom sub-agent requires a non-empty allowed_tools list"
            ));
        }
        return Ok(Some(deduped));
    }

    if matches!(agent_type, SubAgentType::Custom) {
        return Err(anyhow!(
            "Custom sub-agent requires a non-empty allowed_tools list"
        ));
    }

    // Default: full registry inheritance from the parent. The child sees every
    // tool the parent has, including the sub-agent management family. The
    // registry execution guard still blocks approval-gated tools unless the
    // parent runtime is auto-approved.
    Ok(None)
}

/// When a child agent fails because its model is unavailable under the current
/// access profile, a bare provider 403/404 (classified `Authorization` or
/// `State`) is unactionable. Annotate it so the parent knows the likely cause
/// and how to recover (#2653) without re-classifying the underlying error.
fn annotate_child_model_error(err: &str, model: &str) -> String {
    match crate::error_taxonomy::classify_error_message(err) {
        crate::error_taxonomy::ErrorCategory::Authorization
        | crate::error_taxonomy::ErrorCategory::State => format!(
            "{err}\n(child model `{model}` may be unavailable under the current access profile — \
             remove the explicit child model override or adjust child-agent model config before retrying)"
        ),
        _ => {
            // #3020 (#2653): Provider rejections like "Model Not Exist" or
            // "does not exist or you do not have access" often classify as
            // `Internal` rather than `Authorization`/`State`.  Catch these
            // patterns in the raw error text and annotate anyway.
            let lower = err.to_ascii_lowercase();
            if lower.contains("model not exist")
                || lower.contains("model_not_found")
                || lower.contains("does not exist")
                || lower.contains("no such model")
                || lower.contains("invalid model")
            {
                format!(
                    "{err}\n(child model `{model}` may be unavailable under the current access profile — \
                     remove the explicit child model override or adjust child-agent model config before retrying)"
                )
            } else {
                err.to_string()
            }
        }
    }
}

/// Char budget above which a sub-agent summary is treated as a large dump and
/// head+tail truncated. Mirrors `TOOL_RESULT_SENT_CHAR_BUDGET` in
/// `crates/tui/src/client/chat.rs:702` so sub-agent summaries use the same
/// threshold as regular tool outputs. Duplicated locally to avoid coupling the
/// sub-agent module to the wire-compaction internals.
const SUBAGENT_SUMMARY_CHAR_BUDGET: usize = 12_000;
/// Head/tail slice sizes when truncating; mirror the wire constants
/// (`TOOL_RESULT_HEAD_CHARS`/`TOOL_RESULT_TAIL_CHARS`, chat.rs:703-704).
const SUBAGENT_SUMMARY_HEAD_CHARS: usize = 4_000;
const SUBAGENT_SUMMARY_TAIL_CHARS: usize = 4_000;

/// Maximum total characters for all child sub-agent completion events injected
/// into a single turn.  When exceeded, oldest events are dropped with a note.
/// Set to 4× the per-event budget so at most 4 full-size events fit.
const MAX_SUBAGENT_INJECTION_CHARS: usize = 48_000;

/// One-line provenance suffix reinforcing that a sub-agent summary is a
/// self-report (issue #2652). Appended only when the summary was NOT
/// length-truncated, so every summary carries exactly one boundary marker.
const SUBAGENT_SELF_REPORT_NOTE: &str = include_str!("../../prompts/subagent_self_report_note.md");

/// Stamp a sub-agent summary with a provenance/clip marker (issue #2652).
///
/// Returns `(stamped_summary, truncated)`:
/// - When the raw summary is within the budget, append the soft self-report
///   note and report `truncated: false`.
/// - When it exceeds the budget, keep a head+tail slice and stamp it with the
///   existing `[Output truncated ...]` vocabulary (reused from tool-output
///   truncation), adapted to be honest that the elided middle is NOT in the
///   spillover store — there is no `retrieve_tool_result` handle for
///   sub-agent summaries. Report `truncated: true`.
///
/// Every summary therefore gets exactly one boundary marker, never both.
fn stamp_subagent_summary(raw: &str) -> (String, bool) {
    let total = raw.chars().count();
    if total <= SUBAGENT_SUMMARY_CHAR_BUDGET {
        return (format!("{raw}{SUBAGENT_SELF_REPORT_NOTE}"), false);
    }
    let chars: Vec<char> = raw.chars().collect();
    let head: String = chars.iter().take(SUBAGENT_SUMMARY_HEAD_CHARS).collect();
    let tail: String = chars
        .iter()
        .skip(total.saturating_sub(SUBAGENT_SUMMARY_TAIL_CHARS))
        .collect();
    let omitted = total
        .saturating_sub(SUBAGENT_SUMMARY_HEAD_CHARS)
        .saturating_sub(SUBAGENT_SUMMARY_TAIL_CHARS);
    let stamped = format!(
        "{head}\n\n[Sub-agent summary truncated: {SUBAGENT_SUMMARY_HEAD_CHARS} + {SUBAGENT_SUMMARY_TAIL_CHARS} of {total} \
chars shown. This is the child's self-report; the elided middle ({omitted} chars) is not in \
the spillover store and cannot be retrieved via retrieve_tool_result. Re-open the child or \
read changed files directly to verify material claims.]\n\n{tail}",
    );
    (stamped, true)
}

fn summarize_subagent_result(result: &SubAgentResult) -> String {
    if let Some(needs_input) = result.needs_input.as_ref() {
        return format!("Needs input: {}", needs_input.question);
    }
    match (&result.status, result.result.as_ref()) {
        (SubAgentStatus::Completed, Some(text)) => text.clone(),
        (SubAgentStatus::Completed, None) => "Completed (no output)".to_string(),
        (SubAgentStatus::Interrupted(error), _) => format!("Interrupted: {error}"),
        (SubAgentStatus::Cancelled, _) => "Cancelled".to_string(),
        (SubAgentStatus::BudgetExhausted, _) => "Token budget exhausted".to_string(),
        (SubAgentStatus::Failed(error), _) => format!("Failed: {error}"),
        (SubAgentStatus::Running, _) => "Running".to_string(),
    }
}

fn subagent_status_name(status: &SubAgentStatus) -> &'static str {
    match status {
        SubAgentStatus::Running => "running",
        SubAgentStatus::Completed => "completed",
        SubAgentStatus::Interrupted(_) => "interrupted",
        SubAgentStatus::Failed(_) => "failed",
        SubAgentStatus::Cancelled => "cancelled",
        SubAgentStatus::BudgetExhausted => "budget_exhausted",
    }
}

const SUBAGENT_OUTPUT_FORMAT: &str = include_str!("../../prompts/subagent_output_format.md");

const GENERAL_AGENT_INTRO: &str = concat!(
    "You are a trusted general-purpose sub-agent. Your job is to complete the one task you were given, end-to-end, and report back concisely.\n",
    "Stay inside the assigned scope; put adjacent work under RISKS/BLOCKERS.\n",
    "For genuinely multi-step work, track progress with `checklist_write` (and `update_plan` for complex strategy); skip it for short, focused tasks.\n",
    "**Stop quickly on failure**: if the same tool call fails 2 times in a row, stop retrying and return what you have so far with a one-line note explaining what's missing. Do not loop on impossible queries (e.g. external API unreachable, rate-limited, or returning empty).\n",
    "For implementer or repair-style work, keep going within the assigned scope; checkpoint before broadening the task or after repeated failures instead of forcing a tiny tool-call cap.\n\n"
);

const EXPLORE_AGENT_INTRO: &str = concat!(
    "You are a trusted exploration sub-agent (role: `explore`). Your job is to map the relevant code quickly and stay strictly read-only.\n",
    "Default to `EFFORT: quick`: aim for about 3-5 tool calls unless the brief explicitly asks for more.\n",
    "Orient first: confirm the workspace/project root, read relevant AGENTS.md/README guidance when the tree is unfamiliar, then search only the likely scope.\n",
    "Use list_dir/file_search, grep_files, and read_file; use RLM only for long inputs or many semantic slices, not basic path discovery.\n",
    "Honor QUESTION, SCOPE, ALREADY_KNOWN, and STOP_CONDITION. Do not repeat ALREADY_KNOWN work unless evidence contradicts it; do not broaden once QUESTION is answered.\n",
    "DeepSeek V4 can hold broad evidence, but your value is compressed reconnaissance: cite `path:line-range` for each finding and stop once evidence is sufficient. Return partial findings if the next step would be speculative or duplicative.\n",
    "CHANGES will almost always be \"None.\" for an explorer.\n\n"
);

const PLAN_AGENT_INTRO: &str = concat!(
    "You are a trusted planning sub-agent (role: `plan`). Your job is to produce a grounded, prioritized plan, not patches.\n",
    "Read enough code to avoid guessing; each step names its artifact and verification.\n",
    "Use update_plan/checklist_write for plan artifacts and explain key trade-offs.\n",
    "CHANGES should list plan artifacts only, not future speculative edits.\n\n"
);

const REVIEW_AGENT_INTRO: &str = concat!(
    "You are a trusted code review sub-agent (role: `review`). Your job is to find and report severity-scored issues, and stay strictly read-only.\n",
    "Read the diff/files, grep sibling patterns/tests, then order EVIDENCE by severity.\n",
    "Use BLOCKER/MAJOR/MINOR/NIT and include path:line-range plus suggested fix.\n",
    "You may use more tool calls than quick exploration, but stop after decisive evidence instead of widening the review forever.\n",
    "If no MAJOR+ issues exist, say so plainly in SUMMARY.\n",
    "CHANGES will almost always be \"None.\" for a reviewer.\n\n"
);

const CUSTOM_AGENT_INTRO: &str = concat!(
    "You are a trusted custom sub-agent (role: `custom`) with a narrowed tool registry. Your job is to stay tightly scoped to the assigned objective.\n",
    "Use only tools available at runtime; put missing capabilities under BLOCKERS and stop.\n\n"
);

const IMPLEMENTER_AGENT_INTRO: &str = concat!(
    "You are a trusted implementation sub-agent (role: `implementer`). Your job is to land the assigned change with minimal surrounding edits.\n",
    "Read target files before editing; prefer edit_file for narrow changes and apply_patch for hunks.\n",
    "Run relevant verification after edit batches; write needed tests with the implementation.\n",
    "You are not limited to an explorer-style 3-5 tool-call cap. Checkpoint before expanding scope or after repeated failures, then continue only inside the assigned brief.\n",
    "CHANGES is load-bearing: list every modified file with a one-line why.\n\n"
);

const VERIFIER_AGENT_INTRO: &str = concat!(
    "You are a trusted verification sub-agent (role: `verifier`). Your job is to run the requested gates and report results, and stay read-only.\n",
    "Report PASS/FAIL/FLAKY at the top of SUMMARY with exact command evidence.\n",
    "Capture failing assertion and file:line; put obvious fixes under RISKS.\n",
    "You may use more tool calls than quick exploration, but stop after decisive pass/fail evidence.\n",
    "CHANGES will almost always be \"None.\" for a verifier.\n\n"
);
