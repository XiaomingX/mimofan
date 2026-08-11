//! Tool specification traits for the mimofan agent system.
//!
//! This module defines the core abstractions for tools:
//! - `ToolSpec`: The main trait that all tools must implement
//! - `ToolContext`: Execution context passed to tools
//! - `ToolResult`: Unified result type for tool execution
//! - `ToolCapability`: Capabilities and requirements of tools

use std::collections::HashMap;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::features::Features;
use crate::lsp::LspManager;
use crate::network_policy::NetworkPolicyDecider;
use crate::rlm::session::SessionObjectSnapshot;
use crate::rlm::session::{SharedRlmSessionStore, new_shared_rlm_session_store};
use crate::sandbox::backend::SandboxBackend;
use crate::tools::handle::{SharedHandleStore, new_shared_handle_store};
use crate::tools::shell::{SharedShellManager, new_shared_shell_manager};
use crate::worker_profile::ShellPolicy;
pub use mimofan_tools::{
    ApprovalRequirement, ToolCapability, ToolError, ToolResult, optional_bool, optional_str,
    optional_u64, required_str, required_u64,
};

/// Universal default timeout (in ms) for network tools (fetch_url, web_run, web_search).
pub const DEFAULT_NETWORK_TIMEOUT_MS: u64 = 15_000;

#[async_trait]
pub trait DynamicToolExecutor: Send + Sync {
    async fn execute_dynamic_tool(
        &self,
        thread_id: Option<String>,
        namespace: Option<String>,
        name: String,
        input: Value,
    ) -> Result<ToolResult, ToolError>;
}

/// Optional durable runtime services made available to model-visible tools.
///
/// These are intentionally optional so existing unit tests and one-off tool
/// contexts keep working. Tools that need durable task/automation state fail
/// closed with a clear "not available" error when the relevant service is not
/// attached.
#[derive(Clone)]
pub struct RuntimeToolServices {
    pub shell_manager: Option<SharedShellManager>,
    pub task_manager: Option<crate::task_manager::SharedTaskManager>,
    pub automations: Option<crate::automation_manager::SharedAutomationManager>,
    pub task_data_dir: Option<PathBuf>,
    pub active_task_id: Option<String>,
    pub active_thread_id: Option<String>,
    pub dynamic_tool_executor: Option<Arc<dyn DynamicToolExecutor>>,
    /// Hook executor for `shell_env` injection (#456) and any future
    /// tool-side hook events. `None` outside the live engine — test
    /// contexts that don't care about hooks get a no-op.
    pub hook_executor: Option<std::sync::Arc<crate::hooks::HookExecutor>>,
    /// Per-session backing store for `var_handle` payloads. Cloned tool
    /// contexts share this Arc so handles survive across turns.
    pub handle_store: SharedHandleStore,
    /// Per-session persistent RLM kernels, keyed by caller-chosen context name.
    pub rlm_sessions: SharedRlmSessionStore,
}

impl Default for RuntimeToolServices {
    fn default() -> Self {
        Self {
            shell_manager: None,
            task_manager: None,
            automations: None,
            task_data_dir: None,
            active_task_id: None,
            active_thread_id: None,
            dynamic_tool_executor: None,
            hook_executor: None,
            handle_store: new_shared_handle_store(),
            rlm_sessions: new_shared_rlm_session_store(),
        }
    }
}

impl std::fmt::Debug for RuntimeToolServices {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeToolServices")
            .field("shell_manager", &self.shell_manager.is_some())
            .field("task_manager", &self.task_manager.is_some())
            .field("automations", &self.automations.is_some())
            .field("task_data_dir", &self.task_data_dir)
            .field("active_task_id", &self.active_task_id)
            .field("active_thread_id", &self.active_thread_id)
            .field(
                "dynamic_tool_executor",
                &self.dynamic_tool_executor.is_some(),
            )
            .field("hook_executor", &self.hook_executor.is_some())
            .field("handle_store", &true)
            .field("rlm_sessions", &true)
            .finish()
    }
}

/// Identity of a file's on-disk state, used to detect edits made against
/// stale content. Deliberately excludes observed line ranges so that
/// freshness and coverage can be compared independently.
///
/// `content_hash` is the authoritative field: `len` and `modified` alone miss
/// same-length edits made within the same mtime granularity (a `touch`-style
/// rewrite, or two writes inside one second on a coarse filesystem clock).
/// The metadata fields are retained because they still distinguish files whose
/// contents could not be hashed (unreadable or non-UTF8-safe reads fall back
/// to `None`).
#[derive(Debug, Clone, PartialEq, Eq)]
struct FileIdentity {
    len: u64,
    modified: Option<SystemTime>,
    /// SHA-256 of the file bytes, or `None` when the file could not be read.
    /// Two `None` hashes never compare as a content match on their own — the
    /// `len`/`modified` fields carry the comparison in that degraded case.
    content_hash: Option<[u8; 32]>,
}

/// A single inclusive, 1-based range of lines the caller has actually seen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LineRange {
    start: usize,
    end: usize,
}

/// Why a read-before-write check rejected a mutation. Surfaced to the model as
/// a stable machine-readable `reason` so it can branch on the failure mode
/// instead of pattern-matching English prose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PriorReadViolation {
    /// The file was never read in this session.
    NeverRead,
    /// The file was read, but its on-disk contents changed since.
    Stale,
    /// The file's current state could not be inspected to compare.
    Unverifiable,
    /// The specific lines being edited were never observed.
    UnreadLines,
}

impl PriorReadViolation {
    fn as_str(self) -> &'static str {
        match self {
            Self::NeverRead => "never_read",
            Self::Stale => "stale_content",
            Self::Unverifiable => "unverifiable",
            Self::UnreadLines => "unread_lines",
        }
    }
}

/// Marker prefix for the machine-readable trailer appended to prior-read
/// errors. `ToolError` carries only a `String` payload, so structured fields
/// travel as a single-line JSON object the model (and tests) can locate
/// deterministically without disturbing the human-readable guidance above it.
pub const PRIOR_READ_ERROR_TAG: &str = "prior_read_violation=";

/// Build a prior-read `ToolError` whose message keeps the existing prose
/// recovery guidance and appends a parseable `prior_read_violation={...}`
/// trailer carrying `reason`, `tool`, `path` and the expected/actual state.
fn prior_read_error(
    reason: PriorReadViolation,
    tool: &str,
    path: &Path,
    requested_path: &str,
    prose: &str,
) -> ToolError {
    prior_read_error_with(reason, tool, path, requested_path, prose, &[])
}

/// As [`prior_read_error`], with additional structured fields merged into the
/// JSON trailer (used by coverage failures to report expected/actual lines).
fn prior_read_error_with(
    reason: PriorReadViolation,
    tool: &str,
    path: &Path,
    requested_path: &str,
    prose: &str,
    extra: &[(&str, Value)],
) -> ToolError {
    let mut fields = serde_json::Map::new();
    fields.insert("reason".to_string(), json!(reason.as_str()));
    fields.insert("tool".to_string(), json!(tool));
    fields.insert("path".to_string(), json!(path.display().to_string()));
    fields.insert("requested_path".to_string(), json!(requested_path));
    fields.insert("recovery_tool".to_string(), json!("read_file"));
    for (key, value) in extra {
        fields.insert((*key).to_string(), value.clone());
    }
    let trailer = Value::Object(fields);
    ToolError::execution_failed(format!("{prose}\n{PRIOR_READ_ERROR_TAG}{trailer}"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileReadSnapshot {
    identity: FileIdentity,
    /// Inclusive 1-based line ranges observed so far, kept sorted and
    /// coalesced. `None` means the whole file was observed, which is the
    /// case for unranged reads and for writes that produced the content.
    observed: Option<Vec<LineRange>>,
}

impl FileReadSnapshot {
    fn full(identity: FileIdentity) -> Self {
        Self {
            identity,
            observed: None,
        }
    }

    fn ranged(identity: FileIdentity, start: usize, end: usize) -> Self {
        Self {
            identity,
            observed: Some(vec![LineRange { start, end }]),
        }
    }

    fn covers(&self, start: usize, end: usize) -> bool {
        let Some(ranges) = &self.observed else {
            return true;
        };
        // Every line in start..=end must fall inside some observed range.
        // Ranges are sorted and coalesced, so a single walk suffices.
        let mut cursor = start;
        for range in ranges {
            if range.start > cursor {
                return false;
            }
            if range.end >= cursor {
                cursor = range.end + 1;
            }
            if cursor > end {
                return true;
            }
        }
        cursor > end
    }

    /// Merge a newly observed range into this snapshot, coalescing adjacent
    /// and overlapping spans so repeated paging reads accumulate coverage.
    fn add_range(&mut self, start: usize, end: usize) {
        let Some(ranges) = &mut self.observed else {
            return; // already full coverage
        };
        ranges.push(LineRange { start, end });
        ranges.sort_by_key(|r| r.start);
        let mut merged: Vec<LineRange> = Vec::with_capacity(ranges.len());
        for range in ranges.iter() {
            match merged.last_mut() {
                // `start <= end + 1` also coalesces exactly-adjacent ranges
                // (e.g. 1-200 followed by 201-400) into one span.
                Some(last) if range.start <= last.end.saturating_add(1) => {
                    last.end = last.end.max(range.end);
                }
                _ => merged.push(*range),
            }
        }
        *ranges = merged;
    }

    /// Render observed ranges for error messages, e.g. `1-200, 401-600`.
    fn describe_observed(&self) -> String {
        match &self.observed {
            None => "the entire file".to_string(),
            Some(ranges) if ranges.is_empty() => "no lines".to_string(),
            Some(ranges) => ranges
                .iter()
                .map(|r| format!("{}-{}", r.start, r.end))
                .collect::<Vec<_>>()
                .join(", "),
        }
    }
}

#[derive(Debug, Default)]
pub struct FileReadTracker {
    reads: HashMap<PathBuf, FileReadSnapshot>,
}

pub type SharedFileReadTracker = Arc<Mutex<FileReadTracker>>;

fn new_shared_file_read_tracker() -> SharedFileReadTracker {
    Arc::new(Mutex::new(FileReadTracker::default()))
}

fn file_identity(path: &Path) -> Result<FileIdentity, ToolError> {
    let metadata = fs::metadata(path).map_err(|e| {
        ToolError::execution_failed(format!("Failed to inspect {}: {e}", path.display()))
    })?;
    Ok(FileIdentity {
        len: metadata.len(),
        modified: metadata.modified().ok(),
        content_hash: hash_file_contents(path),
    })
}

/// Hash a file's bytes for staleness detection. Best-effort: an unreadable
/// file yields `None` and the comparison degrades to metadata only, which is
/// the pre-hash behaviour rather than a hard failure.
fn hash_file_contents(path: &Path) -> Option<[u8; 32]> {
    use sha2::{Digest, Sha256};

    let mut file = fs::File::open(path).ok()?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher).ok()?;
    Some(hasher.finalize().into())
}

/// Sandbox policy for command execution.
#[derive(Debug, Clone, Default)]
pub enum SandboxPolicy {
    /// No sandboxing (dangerous but sometimes needed)
    #[default]
    None,
}

/// Context passed to tools during execution.
#[derive(Clone)]
pub struct ToolContext {
    /// The workspace root directory
    pub workspace: PathBuf,
    /// Shared shell manager for background tasks and streaming IO.
    pub shell_manager: SharedShellManager,
    /// Per-session snapshots for files successfully observed by `read_file`.
    /// Mutation tools use this to reject narrow edits against unread or stale
    /// content.
    pub file_read_tracker: SharedFileReadTracker,
    /// Sub-agent that owns tool work started through this context. Root user
    /// turns leave this unset; child contexts stamp it so long-running shell
    /// jobs can be attributed in UI surfaces.
    pub owner_agent_id: Option<String>,
    pub owner_agent_name: Option<String>,
    /// Whether to allow paths outside workspace
    pub trust_mode: bool,
    /// Current sandbox policy
    pub sandbox_policy: SandboxPolicy,
    /// Path for notes file
    pub notes_path: PathBuf,
    /// MCP configuration path
    pub mcp_config_path: PathBuf,
    /// Explicit skills directory used for model-visible skill discovery.
    pub skills_dir: Option<PathBuf>,
    /// Restrict skill discovery to mimofan-owned roots plus `skills_dir`.
    pub skills_scan_mimofan_only: bool,
    /// Elevated sandbox policy override (used when retrying after sandbox denial).
    /// This overrides the default sandbox behavior for shell commands.
    pub elevated_sandbox_policy: Option<crate::sandbox::SandboxPolicy>,
    /// Optional user-facing hint for shell commands that fail because the
    /// active sandbox policy intentionally denies outbound network access.
    pub shell_network_denied_hint: Option<String>,
    /// Whether tools should auto-approve without safety checks (YOLO mode).
    /// When true, command safety analysis is skipped for shell execution.
    pub auto_approve: bool,
    /// Effective shell policy for this execution context.
    pub shell_policy: ShellPolicy,
    /// Effective feature flag set for the running session.
    pub features: Features,
    /// Namespace for tool state that should be scoped to the current session/thread.
    pub state_namespace: String,
    /// User-trusted external paths the agent may read/write even when they
    /// fall outside `workspace`. Loaded from `~/.mimofan/workspace-trust.json`
    /// and refreshed when the user runs `/trust add <path>`. Distinct from
    /// `trust_mode`, which is the all-or-nothing legacy switch (#29).
    pub trusted_external_paths: Vec<PathBuf>,
    /// Whether to follow symbolic links during file discovery and tool
    /// operations. When `true`, symlinked directories are traversed and
    /// symlinked paths that resolve outside the workspace are still allowed
    /// (the symlink itself must be inside the workspace). Mirrors the
    /// `workspace_follow_symlinks` setting.
    pub follow_symlinks: bool,
    /// Per-domain network policy (#135). When `None`, network tools fall back
    /// to a permissive default that mirrors pre-v0.7.0 behavior so tests and
    /// other contexts that don't construct a real policy keep working.
    pub network_policy: Option<NetworkPolicyDecider>,
    /// Durable runtime services for task, gate, PR-attempt, GitHub evidence,
    /// and automation tools.
    pub runtime: RuntimeToolServices,
    /// Snapshot of the active prompt/session/history exposed as symbolic RLM
    /// objects. Tools only receive compact cards unless explicitly opening a
    /// bounded object through `rlm_open`.
    pub session_objects: Option<SessionObjectSnapshot>,
    /// Cancellation token for the active engine turn. Tools that may wait on
    /// external work should observe this so UI cancel can interrupt them.
    pub cancel_token: Option<CancellationToken>,
    /// Optional external sandbox backend for shell execution.
    /// When set, exec_shell routes commands through this instead of spawning
    /// a local process.
    pub sandbox_backend: Option<std::sync::Arc<dyn SandboxBackend>>,
    /// Path to the user memory directory (`~/.mimofan/memory/`). `None` when
    /// the user-memory feature (#489) is disabled — tools that read or write
    /// memory should short-circuit on `None`.
    pub memory_dir: Option<PathBuf>,
    /// LSP manager for post-edit diagnostics injection (#428). `None` when
    /// LSP is disabled or the context is constructed in a test that does not
    /// need diagnostics. Edit tools append a `<diagnostics>` block to their
    /// result when this is present and the manager is enabled.
    pub lsp_manager: Option<Arc<LspManager>>,

    /// Large-output router (#548). When `Some`, tool results that exceed the
    /// configured token threshold are routed through a V4-Flash synthesis
    /// sub-agent before being returned to the parent context. `None` disables
    /// routing (e.g. in sub-agents and test contexts to avoid recursion).
    pub large_output_router: Option<crate::tools::large_output_router::LargeOutputRouter>,

    /// Which search backend `web_search` should use. Default: DuckDuckGo. Set via
    /// `[search] provider` in config.toml.
    pub search_provider: crate::config::SearchProvider,
    /// API key for Tavily, Bocha, Metaso, or Baidu. `None` for Bing or DuckDuckGo.
    /// Metaso also falls back to `METASO_API_KEY` env var, then a built-in key.
    /// Baidu also falls back to `BAIDU_SEARCH_API_KEY`.
    pub search_api_key: Option<String>,
    /// Optional DuckDuckGo-compatible HTML endpoint override for `web_search`.
    pub search_base_url: Option<String>,

    /// Per-session workshop variable store (#548). Holds the raw content of
    /// the most recent large-tool routing event so the parent can call
    /// `promote_to_context` later. `None` when the router is disabled.
    pub workshop_vars: Option<
        std::sync::Arc<tokio::sync::Mutex<crate::tools::large_output_router::WorkshopVariables>>,
    >,
}

impl ToolContext {
    /// Create a new `ToolContext` with default settings.
    #[must_use]
    pub fn new(workspace: impl Into<PathBuf>) -> Self {
        let workspace = workspace.into();
        let shell_manager = new_shared_shell_manager(workspace.clone());
        let notes_path = mimofan_config::resolve_project_state_dir(&workspace, "notes.md")
            .expect("hardcoded project notes state path is valid");
        let mcp_config_path = mimofan_config::resolve_project_state_dir(&workspace, "mcp.json")
            .expect("hardcoded project MCP state path is valid");
        Self {
            workspace,
            shell_manager,
            file_read_tracker: new_shared_file_read_tracker(),
            owner_agent_id: None,
            owner_agent_name: None,
            trust_mode: false,
            sandbox_policy: SandboxPolicy::None,
            notes_path,
            mcp_config_path,
            skills_dir: None,
            skills_scan_mimofan_only: false,
            elevated_sandbox_policy: None,
            shell_network_denied_hint: None,
            auto_approve: false,
            shell_policy: ShellPolicy::Full,
            features: Features::with_defaults(),
            state_namespace: "workspace".to_string(),
            trusted_external_paths: Vec::new(),
            follow_symlinks: false,
            network_policy: None,
            runtime: RuntimeToolServices::default(),
            session_objects: None,
            cancel_token: None,
            sandbox_backend: None,
            memory_dir: None,
            lsp_manager: None,
            large_output_router: None,
            search_provider: crate::config::SearchProvider::default(),
            search_api_key: None,
            search_base_url: None,
            workshop_vars: None,
        }
    }

    /// Create a `ToolContext` with all settings specified.
    pub fn with_options(
        workspace: impl Into<PathBuf>,
        trust_mode: bool,
        notes_path: impl Into<PathBuf>,
        mcp_config_path: impl Into<PathBuf>,
    ) -> Self {
        let workspace = workspace.into();
        let shell_manager = new_shared_shell_manager(workspace.clone());
        Self {
            workspace,
            shell_manager,
            file_read_tracker: new_shared_file_read_tracker(),
            owner_agent_id: None,
            owner_agent_name: None,
            trust_mode,
            sandbox_policy: SandboxPolicy::None,
            notes_path: notes_path.into(),
            mcp_config_path: mcp_config_path.into(),
            skills_dir: None,
            skills_scan_mimofan_only: false,
            elevated_sandbox_policy: None,
            shell_network_denied_hint: None,
            auto_approve: false,
            shell_policy: ShellPolicy::Full,
            features: Features::with_defaults(),
            state_namespace: "workspace".to_string(),
            trusted_external_paths: Vec::new(),
            follow_symlinks: false,
            network_policy: None,
            runtime: RuntimeToolServices::default(),
            session_objects: None,
            cancel_token: None,
            sandbox_backend: None,
            memory_dir: None,
            lsp_manager: None,
            large_output_router: None,
            search_provider: crate::config::SearchProvider::default(),
            search_api_key: None,
            search_base_url: None,
            workshop_vars: None,
        }
    }

    /// Create a `ToolContext` with auto-approve mode (YOLO).
    pub fn with_auto_approve(
        workspace: impl Into<PathBuf>,
        trust_mode: bool,
        notes_path: impl Into<PathBuf>,
        mcp_config_path: impl Into<PathBuf>,
        auto_approve: bool,
    ) -> Self {
        let workspace = workspace.into();
        let shell_manager = new_shared_shell_manager(workspace.clone());
        Self {
            workspace,
            shell_manager,
            file_read_tracker: new_shared_file_read_tracker(),
            owner_agent_id: None,
            owner_agent_name: None,
            trust_mode,
            sandbox_policy: SandboxPolicy::None,
            notes_path: notes_path.into(),
            mcp_config_path: mcp_config_path.into(),
            skills_dir: None,
            skills_scan_mimofan_only: false,
            elevated_sandbox_policy: None,
            shell_network_denied_hint: None,
            auto_approve,
            shell_policy: ShellPolicy::Full,
            features: Features::with_defaults(),
            state_namespace: "workspace".to_string(),
            trusted_external_paths: Vec::new(),
            follow_symlinks: false,
            network_policy: None,
            runtime: RuntimeToolServices::default(),
            session_objects: None,
            cancel_token: None,
            sandbox_backend: None,
            memory_dir: None,
            lsp_manager: None,
            large_output_router: None,
            search_provider: crate::config::SearchProvider::default(),
            search_api_key: None,
            search_base_url: None,
            workshop_vars: None,
        }
    }

    /// Attach a per-domain network policy to this context (#135).
    #[must_use]
    pub fn with_network_policy(mut self, policy: NetworkPolicyDecider) -> Self {
        self.network_policy = Some(policy);
        self
    }

    /// Attach durable runtime services to tools.
    #[must_use]
    pub fn with_runtime_services(mut self, runtime: RuntimeToolServices) -> Self {
        self.runtime = runtime;
        self
    }

    /// Stamp tool work with the sub-agent that owns it.
    #[must_use]
    pub fn with_owner_agent(
        mut self,
        agent_id: impl Into<String>,
        agent_name: impl Into<String>,
    ) -> Self {
        let agent_id = agent_id.into();
        let agent_name = agent_name.into();
        self.owner_agent_id = (!agent_id.trim().is_empty()).then_some(agent_id);
        self.owner_agent_name = (!agent_name.trim().is_empty()).then_some(agent_name);
        self
    }

    /// Attach skill discovery settings for tools that need to resolve
    /// model-visible skills by name.
    #[must_use]
    pub fn with_skills_config(
        mut self,
        skills_dir: impl Into<PathBuf>,
        scan_mimofan_only: bool,
    ) -> Self {
        self.skills_dir = Some(skills_dir.into());
        self.skills_scan_mimofan_only = scan_mimofan_only;
        self
    }

    /// Attach active prompt/history/session symbolic objects for RLM tools.
    #[must_use]
    pub fn with_session_objects(mut self, snapshot: SessionObjectSnapshot) -> Self {
        self.session_objects = Some(snapshot);
        self
    }

    /// Attach the active engine cancellation token.
    #[must_use]
    pub fn with_cancel_token(mut self, cancel_token: CancellationToken) -> Self {
        self.cancel_token = Some(cancel_token);
        self
    }

    /// Attach the effective shell policy for this turn.
    #[must_use]
    pub fn with_shell_policy(mut self, policy: ShellPolicy) -> Self {
        self.shell_policy = policy;
        self
    }

    /// Attach an external sandbox backend for remote shell execution.
    #[must_use]
    pub fn with_sandbox_backend(mut self, backend: std::sync::Arc<dyn SandboxBackend>) -> Self {
        self.sandbox_backend = Some(backend);
        self
    }

    /// Set the user's trusted external paths (loaded from
    /// `~/.mimofan/workspace-trust.json`). See [`Self::resolve_path`] for
    /// how the list is consulted.
    #[must_use]
    pub fn with_trusted_external_paths(mut self, paths: Vec<PathBuf>) -> Self {
        self.trusted_external_paths = paths;
        self
    }

    /// Set whether tools should follow symbolic links. When `true`,
    /// `resolve_path` allows symlinked paths that resolve outside the
    /// workspace, and walk-based tools traverse symlinked directories.
    /// Mirrors the `workspace_follow_symlinks` setting.
    #[must_use]
    pub fn with_follow_symlinks(mut self, follow: bool) -> Self {
        self.follow_symlinks = follow;
        self
    }

    /// Attach an LSP manager so that edit tools can auto-inject diagnostics
    /// into their results after a successful file modification (#428).
    #[must_use]
    pub fn with_lsp_manager(mut self, manager: Arc<LspManager>) -> Self {
        self.lsp_manager = Some(manager);
        self
    }

    /// Remember that the caller has observed the current on-disk state of a
    /// file. This is intentionally best-effort so successful reads/writes do
    /// not fail after completing only because a post-operation metadata lookup
    /// raced with filesystem changes.
    pub fn note_file_read(&self, path: &Path) {
        let Ok(identity) = file_identity(path) else {
            return;
        };
        let Ok(mut tracker) = self.file_read_tracker.lock() else {
            return;
        };
        tracker
            .reads
            .insert(path.to_path_buf(), FileReadSnapshot::full(identity));
    }

    /// Remember that the caller observed only lines `start..=end` (1-based,
    /// inclusive) of a file. Repeated ranged reads of the same unchanged file
    /// accumulate, so paging through a file eventually grants full coverage.
    ///
    /// If the file changed since the previous snapshot, prior ranges are
    /// discarded — line numbers from the old content no longer describe what
    /// is on disk now.
    pub fn note_file_read_range(&self, path: &Path, start: usize, end: usize) {
        let Ok(identity) = file_identity(path) else {
            return;
        };
        let Ok(mut tracker) = self.file_read_tracker.lock() else {
            return;
        };
        match tracker.reads.get_mut(path) {
            Some(existing) if existing.identity == identity => {
                existing.add_range(start, end);
            }
            slot => {
                let fresh = FileReadSnapshot::ranged(identity, start, end);
                match slot {
                    Some(existing) => *existing = fresh,
                    None => {
                        tracker.reads.insert(path.to_path_buf(), fresh);
                    }
                }
            }
        }
    }

    /// Require a successful, still-fresh `read_file` snapshot before a narrow
    /// in-place edit. This catches model edits made against guessed or stale
    /// content while leaving transactional patch preflight separate.
    pub fn require_fresh_file_read(
        &self,
        path: &Path,
        requested_path: &str,
    ) -> Result<(), ToolError> {
        self.require_fresh_file_read_for("edit_file", path, requested_path)
    }

    /// Same guarantee as [`Self::require_fresh_file_read`], but attributed to
    /// an arbitrary mutating tool so `write_file`, `apply_patch` and
    /// `notebook_edit` can enforce read-before-write with accurate recovery
    /// instructions instead of telling the model to retry `edit_file`.
    ///
    /// Callers must exempt file *creation* themselves: there is nothing to
    /// read before a new file exists, so gating creation would break normal
    /// use. Only pass paths that already exist on disk.
    pub fn require_fresh_file_read_for(
        &self,
        tool: &str,
        path: &Path,
        requested_path: &str,
    ) -> Result<(), ToolError> {
        let prior = {
            let tracker = self.file_read_tracker.lock().map_err(|_| {
                ToolError::execution_failed(
                    "Failed to check read-before-edit state: tracker lock poisoned".to_string(),
                )
            })?;
            tracker.reads.get(path).cloned()
        };

        let Some(prior) = prior else {
            return Err(prior_read_error(
                PriorReadViolation::NeverRead,
                tool,
                path,
                requested_path,
                &format!(
                    "Refusing {tool} for {} because it has not been read in this session. \
                     Recovery: call read_file with path=\"{requested_path}\" to inspect the current contents, \
                     then retry {tool}.",
                    path.display()
                ),
            ));
        };

        let current = file_identity(path).map_err(|e| {
            prior_read_error(
                PriorReadViolation::Unverifiable,
                tool,
                path,
                requested_path,
                &format!(
                    "Refusing {tool} for {} because the file could not be checked for staleness ({e}). \
                     Recovery: call read_file with path=\"{requested_path}\" again, then retry {tool}.",
                    path.display()
                ),
            )
        })?;

        if current != prior.identity {
            return Err(prior_read_error(
                PriorReadViolation::Stale,
                tool,
                path,
                requested_path,
                &format!(
                    "Refusing {tool} for {} because it changed since the last read_file call. \
                     Recovery: call read_file with path=\"{requested_path}\" again and retry with the current contents.",
                    path.display()
                ),
            ));
        }

        Ok(())
    }

    /// Require that the lines being edited were actually observed by a prior
    /// `read_file`. A partial read of lines 1-200 must not authorize a blind
    /// edit of line 800: the model has never seen that content.
    ///
    /// `start` and `end` are 1-based inclusive line numbers of the edit
    /// target. Callers must have already passed `require_fresh_file_read`.
    pub fn require_read_coverage(
        &self,
        path: &Path,
        requested_path: &str,
        start: usize,
        end: usize,
    ) -> Result<(), ToolError> {
        let prior = {
            let tracker = self.file_read_tracker.lock().map_err(|_| {
                ToolError::execution_failed(
                    "Failed to check read-before-edit state: tracker lock poisoned".to_string(),
                )
            })?;
            tracker.reads.get(path).cloned()
        };

        // Absence is handled by `require_fresh_file_read`; don't double-report.
        let Some(prior) = prior else {
            return Ok(());
        };

        if prior.covers(start, end) {
            return Ok(());
        }

        let target = if start == end {
            format!("line {start}")
        } else {
            format!("lines {start}-{end}")
        };
        let span = end.saturating_sub(start).saturating_add(1);
        let suggested_max = span.max(50);
        let observed = prior.describe_observed();
        Err(prior_read_error_with(
            PriorReadViolation::UnreadLines,
            "edit_file",
            path,
            requested_path,
            &format!(
                "Refusing edit_file for {} because {target} was never read in this session. \
                 Only these lines have been read: {observed}. Editing unread lines risks overwriting content you have not seen. \
                 Recovery: call read_file with path=\"{requested_path}\" start_line={start} max_lines={suggested_max} \
                 to inspect the target region, then retry the same edit_file call.",
                path.display(),
            ),
            &[
                ("expected_lines_read", json!(format!("{start}-{end}"))),
                ("actual_lines_read", json!(observed)),
                ("edit_start_line", json!(start)),
                ("edit_end_line", json!(end)),
            ],
        ))
    }

    /// Resolve a path relative to workspace, validating it doesn't escape.
    ///
    /// This handles both existing files (using canonicalize) and non-existent files
    /// (for write operations) by canonicalizing the parent directory and appending
    /// the filename.
    /// Resolve a path relative to workspace, validating it doesn't escape.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use crate::tools::spec::ToolContext;
    /// let ctx = ToolContext::new(".");
    /// let path = ctx.resolve_path("README.md")?;
    /// # Ok::<(), crate::tools::spec::ToolError>(())
    /// ```
    pub fn resolve_path(&self, raw: &str) -> Result<PathBuf, ToolError> {
        let candidate = if std::path::Path::new(raw).is_absolute() {
            PathBuf::from(raw)
        } else {
            self.workspace.join(raw)
        };

        // NOTE: trust mode only bypasses the *approval prompt* — it must NOT
        // bypass workspace boundary validation. Decoupling "skip confirmation"
        // from "skip path sandbox" prevents a信任模式 session from reading or
        // writing arbitrary filesystem locations (e.g. /etc/shadow, ~/.ssh).
        // The escape check below (and the trusted-external-path allowlist) runs
        // unconditionally regardless of `trust_mode`.

        // Try to canonicalize the workspace
        let workspace_canonical = self
            .workspace
            .canonicalize()
            .unwrap_or_else(|_| self.workspace.clone());

        // When follow_symlinks is enabled, check the non-canonical (symlink)
        // path against the workspace first. A symlink inside the workspace
        // that resolves outside is allowed — the symlink itself is the gate.
        if self.follow_symlinks {
            let candidate_normalized = normalize_path(&candidate);
            let workspace_normalized = normalize_path(&self.workspace);
            let workspace_canonical_normalized = normalize_path(&workspace_canonical);

            if candidate_normalized.starts_with(&workspace_normalized)
                || candidate_normalized.starts_with(&workspace_canonical_normalized)
            {
                // The symlink (or plain path) is inside the workspace.
                // Return the canonicalized target so file I/O works correctly.
                if candidate.exists() {
                    return Ok(candidate.canonicalize().unwrap_or(candidate));
                }
                // Non-existent path: canonicalize the deepest existing ancestor
                return self.resolve_nonexistent_path(candidate, &workspace_canonical);
            }

            // Path is outside workspace even before resolving symlinks.
            // Fall through to the standard escape check.
        }

        // For the initial check, also try to canonicalize the candidate if possible
        // This handles symlinks like /var -> /private/var on macOS
        let candidate_canonical = candidate
            .canonicalize()
            .unwrap_or_else(|_| normalize_path(&candidate));
        let workspace_normalized = normalize_path(&workspace_canonical);

        // Check if the candidate is under the workspace (comparing canonical paths)
        if !candidate_canonical.starts_with(&workspace_normalized) {
            // Also try with non-canonical workspace for cases where workspace itself
            // hasn't been canonicalized yet
            let workspace_plain = normalize_path(&self.workspace);
            let candidate_normalized = normalize_path(&candidate);
            if !candidate_normalized.starts_with(&workspace_plain)
                && !self.is_trusted_external_path(&candidate_canonical)
                && !self.is_trusted_external_path(&candidate_normalized)
            {
                return Err(ToolError::PathEscape {
                    path: candidate_canonical,
                });
            }
        }

        // For existing paths, use canonicalize directly
        if candidate.exists() {
            let canonical = candidate.canonicalize().map_err(|e| {
                ToolError::execution_failed(format!(
                    "Failed to canonicalize {}: {}",
                    candidate.display(),
                    e
                ))
            })?;

            if !canonical.starts_with(&workspace_canonical)
                && !self.is_trusted_external_path(&canonical)
            {
                return Err(ToolError::PathEscape { path: canonical });
            }

            return Ok(canonical);
        }

        self.resolve_nonexistent_path(candidate, &workspace_canonical)
    }

    /// Resolve a non-existent path by canonicalizing its deepest existing
    /// ancestor and validating the result is under the workspace or a
    /// trusted external path.
    fn resolve_nonexistent_path(
        &self,
        candidate: PathBuf,
        workspace_canonical: &Path,
    ) -> Result<PathBuf, ToolError> {
        let workspace_normalized = normalize_path(workspace_canonical);
        let workspace_plain = normalize_path(&self.workspace);
        let mut existing_ancestor = candidate.clone();
        let mut suffix_parts: Vec<std::ffi::OsString> = Vec::new();

        while !existing_ancestor.exists() {
            if let Some(file_name) = existing_ancestor.file_name() {
                suffix_parts.push(file_name.to_owned());
            }
            match existing_ancestor.parent() {
                Some(parent) if !parent.as_os_str().is_empty() => {
                    existing_ancestor = parent.to_path_buf();
                }
                _ => {
                    // No existing parent found; fall back to simple check
                    break;
                }
            }
        }
        let ancestor_normalized = normalize_path(&existing_ancestor);

        let canonical_ancestor = if existing_ancestor.exists() {
            existing_ancestor
                .canonicalize()
                .unwrap_or(existing_ancestor)
        } else {
            existing_ancestor
        };

        // Rebuild the full path from canonicalized ancestor
        let mut canonical = canonical_ancestor;
        for part in suffix_parts.into_iter().rev() {
            canonical.push(part);
        }
        let canonical = normalize_path(&canonical);

        if self.follow_symlinks
            && (ancestor_normalized.starts_with(&workspace_plain)
                || ancestor_normalized.starts_with(&workspace_normalized))
        {
            return Ok(canonical);
        }

        // Validate it's under workspace, OR is under a user-trusted external
        // path (`/trust add <path>` from the slash command, persisted in
        // `~/.mimofan/workspace-trust.json`).
        if !canonical.starts_with(workspace_canonical)
            && !canonical.starts_with(&workspace_normalized)
            && !self.is_trusted_external_path(&canonical)
        {
            return Err(ToolError::PathEscape { path: canonical });
        }

        Ok(canonical)
    }

    /// Whether `path` is under any of the user-trusted external roots. The
    /// caller should pass an already-canonicalized (or normalized) path.
    fn is_trusted_external_path(&self, path: &Path) -> bool {
        self.trusted_external_paths
            .iter()
            .any(|trusted| path.starts_with(trusted))
    }

    /// Set the trust mode.
    pub fn with_trust_mode(mut self, trust: bool) -> Self {
        self.trust_mode = trust;
        self
    }

    /// Set the sandbox policy.
    pub fn with_sandbox_policy(mut self, policy: SandboxPolicy) -> Self {
        self.sandbox_policy = policy;
        self
    }

    /// Set feature flags for tool execution.
    pub fn with_features(mut self, features: Features) -> Self {
        self.features = features;
        self
    }

    /// Override the shared shell manager.
    pub fn with_shell_manager(mut self, shell_manager: SharedShellManager) -> Self {
        self.shell_manager = shell_manager;
        self
    }

    /// Set the elevated sandbox policy override.
    ///
    /// This is used when retrying a tool after a sandbox denial, to run
    /// with elevated permissions.
    pub fn with_elevated_sandbox_policy(mut self, policy: crate::sandbox::SandboxPolicy) -> Self {
        self.elevated_sandbox_policy = Some(policy);
        self
    }

    /// Set the shell network-denial hint used by network-restricted modes.
    pub fn with_shell_network_denied_hint(mut self, hint: impl Into<String>) -> Self {
        self.shell_network_denied_hint = Some(hint.into());
        self
    }

    /// Set the namespace used for session-scoped tool state.
    pub fn with_state_namespace(mut self, namespace: impl Into<String>) -> Self {
        self.state_namespace = namespace.into();
        self
    }

    /// Attach the large-output router (#548). When set, tool results that
    /// exceed the configured token threshold are synthesised by a V4-Flash
    /// sub-agent before being returned to the parent context.
    #[must_use]
    pub fn with_large_output_router(
        mut self,
        router: crate::tools::large_output_router::LargeOutputRouter,
        vars: std::sync::Arc<
            tokio::sync::Mutex<crate::tools::large_output_router::WorkshopVariables>,
        >,
    ) -> Self {
        self.large_output_router = Some(router);
        self.workshop_vars = Some(vars);
        self
    }
}

/// Gather LSP diagnostics for `paths` using the manager stored in `context`,
/// and return the rendered `<diagnostics …>` blocks joined by newlines.
///
/// Returns an empty string when:
/// - `context.lsp_manager` is `None`
/// - the manager's `enabled` flag is `false`
/// - none of the files produce diagnostics (e.g. all clean, or language unknown)
///
/// This function is non-blocking by design: every failure mode (missing LSP
/// binary, timeout, unknown language) degrades to an empty string rather than
/// propagating an error to the caller.
pub async fn lsp_diagnostics_for_paths(context: &ToolContext, paths: &[PathBuf]) -> String {
    use crate::lsp::render_blocks;

    let manager = match context.lsp_manager.as_ref() {
        Some(m) if m.config().enabled => m,
        _ => return String::new(),
    };

    let mut blocks = Vec::new();
    for (idx, path) in paths.iter().enumerate() {
        if let Some(block) = manager.diagnostics_for(path, idx as u64).await {
            blocks.push(block);
        }
    }

    render_blocks(&blocks)
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut prefix: Option<std::ffi::OsString> = None;
    let mut is_root = false;
    let mut stack: Vec<std::ffi::OsString> = Vec::new();

    for component in path.components() {
        match component {
            Component::Prefix(prefix_component) => {
                prefix = Some(prefix_component.as_os_str().to_owned());
            }
            Component::RootDir => {
                is_root = true;
            }
            Component::CurDir => {}
            Component::ParentDir => {
                let parent = Component::ParentDir.as_os_str();
                if let Some(last) = stack.pop() {
                    if last == parent {
                        stack.push(last);
                        stack.push(parent.to_owned());
                    }
                } else if !is_root {
                    stack.push(parent.to_owned());
                }
            }
            Component::Normal(part) => {
                stack.push(part.to_owned());
            }
        }
    }

    let mut normalized = PathBuf::new();
    if let Some(prefix) = prefix {
        normalized.push(prefix);
    }
    if is_root {
        normalized.push(Path::new(std::path::MAIN_SEPARATOR_STR));
    }
    for part in stack {
        normalized.push(part);
    }
    normalized
}

/// The core trait that all tools must implement.
#[async_trait]
pub trait ToolSpec: Send + Sync {
    /// Returns the unique name of this tool (used in API calls).
    fn name(&self) -> &str;

    /// Returns a human-readable description of what this tool does.
    fn description(&self) -> &str;

    /// Returns the JSON Schema for the tool's input parameters.
    fn input_schema(&self) -> Value;

    /// Returns the capabilities this tool has.
    fn capabilities(&self) -> Vec<ToolCapability>;

    /// Returns the approval requirement for this tool.
    fn approval_requirement(&self) -> ApprovalRequirement {
        let caps = self.capabilities();
        if caps.contains(&ToolCapability::ExecutesCode) {
            ApprovalRequirement::Required
        } else if caps.contains(&ToolCapability::WritesFiles) {
            ApprovalRequirement::Suggest
        } else {
            ApprovalRequirement::Auto
        }
    }

    /// Returns the approval requirement for this concrete tool input.
    fn approval_requirement_for(&self, _input: &Value) -> ApprovalRequirement {
        self.approval_requirement()
    }

    /// Returns whether this tool is read-only.
    fn is_read_only(&self) -> bool {
        let caps = self.capabilities();
        caps.contains(&ToolCapability::ReadOnly)
            && !caps.contains(&ToolCapability::WritesFiles)
            && !caps.contains(&ToolCapability::ExecutesCode)
    }

    /// Returns whether this concrete tool input is read-only.
    fn is_read_only_for(&self, _input: &Value) -> bool {
        self.is_read_only()
    }

    /// Returns whether this tool can be executed in parallel with others.
    fn supports_parallel(&self) -> bool {
        false
    }

    /// Returns whether this concrete tool input can run in parallel.
    fn supports_parallel_for(&self, _input: &Value) -> bool {
        self.supports_parallel()
    }

    /// Returns whether this input starts durable/detached work and returns
    /// immediately. Detached starts are not read-only, but in auto-approved
    /// turns they do not need to block neighboring read-only inspections.
    fn starts_detached_for(&self, _input: &Value) -> bool {
        false
    }

    /// Returns whether this tool should be excluded from the model-visible
    /// tool catalog (deferred loading). Tools marked `true` are registered
    /// but not sent to the model until explicitly activated via tool search.
    fn defer_loading(&self) -> bool {
        false
    }

    /// Returns whether this tool should be advertised in the model-facing
    /// catalog. Hidden compatibility tools remain registered and executable
    /// by name so saved transcripts can replay without teaching new sessions
    /// the deprecated spelling.
    fn model_visible(&self) -> bool {
        true
    }

    /// Execute the tool with the given input and context.
    async fn execute(&self, input: Value, context: &ToolContext) -> Result<ToolResult, ToolError>;
}

// === Unit Tests ===

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trust_mode_still_enforces_workspace_boundary() {
        // Regression for #733: trust mode must only skip the approval prompt,
        // never the workspace path sandbox. An absolute path outside the
        // workspace (e.g. /etc/passwd) must be rejected even in trust mode.
        let dir = tempfile::TempDir::new().unwrap();
        // Canonicalize the workspace root: on macOS `/var` is a symlink to
        // `/private/var`, and `resolve_path` compares canonicalized paths.
        let ws = dir.path().canonicalize().unwrap_or_else(|_| dir.path().to_path_buf());
        let ctx = ToolContext::new(ws.clone()).with_trust_mode(true);

        let result = ctx.resolve_path("/etc/passwd");
        assert!(
            matches!(result, Err(ToolError::PathEscape { .. })),
            "trust mode must not bypass path boundary validation, got {:?}",
            result
        );

        // A relative path inside the workspace still resolves normally.
        let inside = ctx.resolve_path("README.md").unwrap();
        assert!(inside.starts_with(&ws));
    }

    #[test]
    fn trust_mode_allows_trusted_external_path() {
        // Trusted external roots (added via `/trust`) remain reachable in
        // trust mode — the allowlist is independent of the boundary check.
        let dir = tempfile::TempDir::new().unwrap();
        let trusted = tempfile::TempDir::new().unwrap();
        // The allowlist is compared against canonicalized candidate paths, so
        // push the canonical root (macOS `/var` -> `/private/var`).
        let trusted_root = trusted
            .path()
            .canonicalize()
            .unwrap_or_else(|_| trusted.path().to_path_buf());
        let ws = dir.path().canonicalize().unwrap_or_else(|_| dir.path().to_path_buf());
        let mut ctx = ToolContext::new(ws).with_trust_mode(true);
        ctx.trusted_external_paths.push(trusted_root.clone());

        let inside = ctx
            .resolve_path(trusted_root.join("x.txt").to_str().unwrap())
            .unwrap();
        assert!(inside.starts_with(&trusted_root));
    }

    fn identity() -> FileIdentity {
        FileIdentity {
            len: 42,
            modified: None,
            content_hash: None,
        }
    }

    #[test]
    fn full_snapshot_covers_every_line() {
        let snap = FileReadSnapshot::full(identity());
        assert!(snap.covers(1, 1));
        assert!(snap.covers(900, 1000));
    }

    #[test]
    fn ranged_snapshot_covers_only_observed_lines() {
        let snap = FileReadSnapshot::ranged(identity(), 1, 200);
        assert!(snap.covers(1, 200));
        assert!(snap.covers(150, 200));
        // Straddling the boundary is not covered.
        assert!(!snap.covers(200, 201));
        assert!(!snap.covers(800, 800));
    }

    #[test]
    fn adjacent_ranges_coalesce_into_one_span() {
        let mut snap = FileReadSnapshot::ranged(identity(), 1, 200);
        snap.add_range(201, 400);
        // 1-200 and 201-400 are adjacent, so 1-400 is now fully covered.
        assert!(snap.covers(1, 400));
        assert_eq!(snap.describe_observed(), "1-400");
    }

    #[test]
    fn disjoint_ranges_leave_a_hole() {
        let mut snap = FileReadSnapshot::ranged(identity(), 1, 100);
        snap.add_range(300, 400);
        assert!(snap.covers(1, 100));
        assert!(snap.covers(300, 400));
        // The gap between them is still unread.
        assert!(!snap.covers(200, 200));
        // A span crossing the hole is not covered.
        assert!(!snap.covers(100, 300));
        assert_eq!(snap.describe_observed(), "1-100, 300-400");
    }

    #[test]
    fn overlapping_ranges_merge() {
        let mut snap = FileReadSnapshot::ranged(identity(), 1, 100);
        snap.add_range(50, 150);
        assert!(snap.covers(1, 150));
        assert_eq!(snap.describe_observed(), "1-150");
    }

    #[test]
    fn out_of_order_ranges_still_coalesce() {
        let mut snap = FileReadSnapshot::ranged(identity(), 201, 400);
        snap.add_range(1, 200);
        assert!(snap.covers(1, 400));
        assert_eq!(snap.describe_observed(), "1-400");
    }

    #[test]
    fn adding_range_to_full_snapshot_keeps_full_coverage() {
        let mut snap = FileReadSnapshot::full(identity());
        snap.add_range(1, 10);
        assert!(snap.covers(5000, 5001));
        assert_eq!(snap.describe_observed(), "the entire file");
    }

    #[test]
    fn ranged_read_after_file_change_discards_stale_ranges() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("f.txt");
        std::fs::write(&path, "a\nb\nc\n").unwrap();
        let ctx = ToolContext::new(dir.path().to_path_buf());

        ctx.note_file_read_range(&path, 1, 3);
        // Rewrite with different length so the identity changes.
        std::fs::write(&path, "a\nb\nc\nd\ne\nf\ng\n").unwrap();
        ctx.note_file_read_range(&path, 5, 7);

        // Line 1 was only observed against the *old* content, so its
        // coverage must not carry over.
        let err = ctx.require_read_coverage(&path, "f.txt", 1, 1);
        assert!(err.is_err(), "stale ranges must be discarded on change");
        assert!(ctx.require_read_coverage(&path, "f.txt", 5, 7).is_ok());
    }

    #[test]
    fn coverage_error_names_the_recovery_call() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("f.txt");
        std::fs::write(&path, "x\n".repeat(500)).unwrap();
        let ctx = ToolContext::new(dir.path().to_path_buf());
        ctx.note_file_read_range(&path, 1, 200);

        let err = ctx
            .require_read_coverage(&path, "f.txt", 300, 300)
            .expect_err("line 300 is unread");
        let msg = err.to_string();
        assert!(msg.contains("line 300"), "{msg}");
        assert!(msg.contains("start_line=300"), "{msg}");
        assert!(msg.contains("1-200"), "{msg}");
    }

    #[test]
    fn identity_detects_same_length_same_mtime_rewrite() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("f.txt");
        std::fs::write(&path, "aaaa").unwrap();
        let before = file_identity(&path).unwrap();

        std::fs::write(&path, "bbbb").unwrap();
        let after = file_identity(&path).unwrap();

        // Same byte length: the pre-#695 identity could not tell these apart
        // whenever the mtime also failed to advance.
        assert_eq!(before.len, after.len);
        assert_ne!(
            before.content_hash, after.content_hash,
            "content hash must distinguish same-length rewrites"
        );
        assert_ne!(before, after);
    }

    #[test]
    fn prior_read_error_carries_parseable_reason() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("f.txt");
        std::fs::write(&path, "body\n").unwrap();
        let ctx = ToolContext::new(dir.path().to_path_buf());

        let err = ctx
            .require_fresh_file_read_for("write_file", &path, "f.txt")
            .expect_err("an unread file must be refused");
        let msg = err.to_string();

        // Human-readable guidance is preserved...
        assert!(msg.contains("Recovery: call read_file"), "{msg}");
        // ...and a machine-parseable trailer rides along with it.
        let trailer = msg
            .split_once(PRIOR_READ_ERROR_TAG)
            .expect("structured trailer must be present")
            .1;
        let parsed: Value = serde_json::from_str(trailer.trim()).expect("trailer must be JSON");
        assert_eq!(parsed["reason"], "never_read");
        assert_eq!(parsed["tool"], "write_file");
        assert_eq!(parsed["requested_path"], "f.txt");
        assert_eq!(parsed["recovery_tool"], "read_file");
    }

    #[test]
    fn stale_content_reason_is_reported_separately_from_never_read() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("f.txt");
        std::fs::write(&path, "one\n").unwrap();
        let ctx = ToolContext::new(dir.path().to_path_buf());

        ctx.note_file_read(&path);
        std::fs::write(&path, "two\n").unwrap();

        let err = ctx
            .require_fresh_file_read_for("apply_patch", &path, "f.txt")
            .expect_err("a changed file must be refused");
        let trailer = err
            .to_string()
            .split_once(PRIOR_READ_ERROR_TAG)
            .expect("structured trailer must be present")
            .1
            .trim()
            .to_string();
        let parsed: Value = serde_json::from_str(&trailer).expect("trailer must be JSON");
        assert_eq!(parsed["reason"], "stale_content");
        assert_eq!(parsed["tool"], "apply_patch");
    }

    #[test]
    fn coverage_error_reports_expected_and_actual_lines() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("f.txt");
        std::fs::write(&path, "x\n".repeat(500)).unwrap();
        let ctx = ToolContext::new(dir.path().to_path_buf());
        ctx.note_file_read_range(&path, 1, 200);

        let err = ctx
            .require_read_coverage(&path, "f.txt", 300, 310)
            .expect_err("line 300 is unread");
        let trailer = err
            .to_string()
            .split_once(PRIOR_READ_ERROR_TAG)
            .expect("structured trailer must be present")
            .1
            .trim()
            .to_string();
        let parsed: Value = serde_json::from_str(&trailer).expect("trailer must be JSON");
        assert_eq!(parsed["reason"], "unread_lines");
        assert_eq!(parsed["expected_lines_read"], "300-310");
        assert_eq!(parsed["actual_lines_read"], "1-200");
    }

    #[test]
    fn unread_file_defers_to_freshness_check() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("f.txt");
        std::fs::write(&path, "x\n").unwrap();
        let ctx = ToolContext::new(dir.path().to_path_buf());
        // No snapshot recorded: coverage stays silent so the caller's
        // read-before-edit error is the one the model sees.
        assert!(ctx.require_read_coverage(&path, "f.txt", 1, 1).is_ok());
    }
}
