//! Request/response types and shared utilities for the runtime API.

use std::path::PathBuf;

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use mimofan_protocol::runtime::RuntimeCapabilities;
use mimofan_protocol::runtime::RuntimeExperimentalCapabilities;

use crate::runtime_threads::types::{ThreadRecord, TurnRecord};
use crate::session_manager::SessionMetadata;
use crate::task_manager::TaskCounts;
use crate::task_manager::TaskSummary;
use crate::tools::subagent::AgentWorkerRecord;

// ── Request types ───────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(crate) struct StreamTurnRequest {
    pub prompt: String,
    pub model: Option<String>,
    pub mode: Option<String>,
    pub workspace: Option<PathBuf>,
    pub allow_shell: Option<bool>,
    pub trust_mode: Option<bool>,
    pub auto_approve: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct CreateSessionRequest {
    pub thread_id: String,
    pub title: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ResumeSessionRequest {
    pub model: Option<String>,
    pub mode: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SaveSessionRequest {
    /// Thread ID to save as a session. If omitted, saves the most recently
    /// active thread.
    #[serde(default)]
    pub thread_id: Option<String>,
    /// If provided, update the existing session with this ID instead of
    /// creating a new one.
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct UndoTurnRequest {
    /// How many turns back to undo (default 0 = last turn only).
    #[serde(default)]
    pub depth: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RetryTurnRequest {
    /// How many turns back to retry (default 0 = last turn only).
    #[serde(default)]
    pub depth: Option<usize>,
    /// Override the user message text. If omitted, the original text
    /// from the dropped turn is re-used.
    #[serde(default)]
    pub prompt: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DecideApprovalBody {
    pub decision: String,
    #[serde(default)]
    pub remember: bool,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SubmitUserInputBody {
    pub answers: Vec<UserInputAnswerBody>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct UserInputAnswerBody {
    pub id: String,
    pub label: String,
    pub value: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SetSkillEnabledRequest {
    pub enabled: bool,
}

// ── Response types ──────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub(crate) struct HealthResponse {
    pub status: &'static str,
    pub service: &'static str,
    pub mode: &'static str,
}

#[derive(Debug, Serialize)]
pub(crate) struct SessionsResponse {
    pub sessions: Vec<SessionMetadata>,
}

#[derive(Debug, Serialize)]
pub(crate) struct SessionDetailResponse {
    pub metadata: SessionMetadata,
    pub messages: Vec<serde_json::Value>,
    pub system_prompt: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct CreateSessionResponse {
    pub session_id: String,
    pub thread_id: String,
    pub message_count: usize,
    pub title: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct ResumeSessionResponse {
    pub thread_id: String,
    pub session_id: String,
    pub message_count: usize,
    pub summary: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct SaveSessionResponse {
    pub session_id: String,
    pub session: SessionDetailResponse,
}

#[derive(Debug, Serialize)]
pub(crate) struct TasksResponse {
    pub tasks: Vec<TaskSummary>,
    pub counts: TaskCounts,
}

#[derive(Debug, Serialize)]
pub(crate) struct ThreadSummary {
    pub id: String,
    pub title: String,
    pub preview: String,
    pub model: String,
    pub mode: String,
    pub workspace: PathBuf,
    pub branch: Option<String>,
    pub head: Option<String>,
    pub dirty: bool,
    pub archived: bool,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub latest_turn_id: Option<String>,
    pub latest_turn_status: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct UndoTurnResponse {
    /// The new forked thread (with the last N turns removed).
    pub thread: ThreadRecord,
    /// The original user message text from the first dropped turn,
    /// so the GUI can pre-populate the input box.
    pub original_user_text: Option<String>,
}

/// Result of the snapshot-based file rollback step of patch-undo.
#[derive(Debug, Serialize)]
pub(crate) struct PatchUndoResult {
    /// Whether files were restored from a snapshot.
    pub files_restored: bool,
    /// Human-readable summary of what was restored (diff stat).
    pub summary: Option<String>,
    /// The label of the restored snapshot.
    pub snapshot_label: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct PatchUndoResponse {
    /// Result of the snapshot-based file rollback step.
    pub patch_result: PatchUndoResult,
    /// The new forked thread (with the last turn removed).
    pub thread: ThreadRecord,
    /// The original user text from the removed turn (for re-editing).
    pub original_user_text: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct RetryTurnResponse {
    /// The new forked thread (with the last N turns removed).
    pub thread: ThreadRecord,
    /// The turn created by the retry.
    pub turn: TurnRecord,
}

#[derive(Debug, Serialize)]
pub(crate) struct StartTurnResponse {
    pub thread: ThreadRecord,
    pub turn: TurnRecord,
}

#[derive(Debug, Serialize)]
pub(crate) struct DecideApprovalResponse {
    pub ok: bool,
    pub approval_id: String,
    pub decision: String,
    pub delivered: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct SubmitUserInputResponse {
    pub ok: bool,
    pub input_id: String,
    pub delivered: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct RuntimeInfoResponse {
    pub service: &'static str,
    pub runtime_api_version: &'static str,
    pub mimofan_version: &'static str,
    pub bind_host: String,
    pub port: u16,
    pub auth_required: bool,
    pub transports: Vec<&'static str>,
    pub capabilities: RuntimeCapabilities,
    pub experimental: RuntimeExperimentalCapabilities,
    // Backward-compatible alias kept for existing clients.
    pub version: &'static str,
}

#[derive(Debug, Serialize)]
pub(crate) struct AgentRunsResponse {
    pub runs: Vec<AgentWorkerRecord>,
}

#[derive(Debug, Serialize)]
pub(crate) struct SkillEntry {
    pub name: String,
    pub description: String,
    pub path: PathBuf,
    pub enabled: bool,
    pub is_bundled: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct SkillsResponse {
    pub directory: PathBuf,
    pub directories: Vec<PathBuf>,
    pub warnings: Vec<String>,
    pub skills: Vec<SkillEntry>,
}

#[derive(Debug, Serialize)]
pub(crate) struct SetSkillEnabledResponse {
    pub name: String,
    pub enabled: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct McpServerEntry {
    pub name: String,
    pub enabled: bool,
    pub required: bool,
    pub command: Option<String>,
    pub url: Option<String>,
    pub connected: bool,
    pub enabled_tools: Vec<String>,
    pub disabled_tools: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct McpServersResponse {
    pub servers: Vec<McpServerEntry>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct McpToolsQuery {
    pub server: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct McpToolEntry {
    pub server: String,
    pub name: String,
    pub prefixed_name: String,
    pub description: Option<String>,
    pub input_schema: Value,
}

#[derive(Debug, Serialize)]
pub(crate) struct McpToolsResponse {
    pub tools: Vec<McpToolEntry>,
}

#[derive(Debug, Serialize)]
pub(crate) struct WorkspaceStatusResponse {
    pub workspace: PathBuf,
    pub git_repo: bool,
    pub branch: Option<String>,
    pub head: Option<String>,
    pub dirty: bool,
    pub staged: usize,
    pub unstaged: usize,
    pub untracked: usize,
    pub ahead: Option<u32>,
    pub behind: Option<u32>,
}

#[derive(Debug, Default)]
pub(crate) struct WorkspaceGitMetadata {
    pub branch: Option<String>,
    pub head: Option<String>,
    pub dirty: bool,
}

#[derive(Debug, Deserialize)]
pub(crate) struct UsageQuery {
    /// ISO-8601 lower bound (inclusive). When omitted, no lower bound.
    pub since: Option<String>,
    /// ISO-8601 upper bound (inclusive). When omitted, no upper bound.
    pub until: Option<String>,
    /// Bucket key. One of `day` (default), `model`, `provider`, `thread`.
    pub group_by: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SnapshotsQuery {
    /// Maximum number of snapshots to return. Mirrors `/restore list [N]`.
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize)]
pub(crate) struct SnapshotEntry {
    pub id: String,
    pub label: String,
    pub timestamp: i64,
}

// ── Query types ─────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(crate) struct SessionsQuery {
    pub limit: Option<usize>,
    pub search: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct TasksQuery {
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ThreadsQuery {
    pub limit: Option<usize>,
    pub include_archived: Option<bool>,
    /// When `true`, returns archived threads only (overrides `include_archived`).
    pub archived_only: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ThreadSummaryQuery {
    pub limit: Option<usize>,
    pub search: Option<String>,
    pub include_archived: Option<bool>,
    /// When `true`, returns archived threads only (overrides `include_archived`).
    pub archived_only: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AutomationRunsQuery {
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ThreadEventsQuery {
    pub since_seq: Option<u64>,
    pub replay_limit: Option<usize>,
}

// ── API error type ──────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub(crate) struct ApiError {
    pub status: StatusCode,
    pub message: String,
}

impl ApiError {
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: message.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({
                "error": {
                    "message": self.message,
                    "status": self.status.as_u16(),
                }
            })),
        )
            .into_response()
    }
}

// ── Shared helper functions ─────────────────────────────────────────

pub(crate) fn default_runtime_capabilities() -> RuntimeCapabilities {
    RuntimeCapabilities {
        threads: true,
        turns: true,
        turn_steer: true,
        turn_interrupt: true,
        event_replay: true,
        external_tools: true,
        environments: false,
        worker_runtime: true,
    }
}

pub(crate) fn runtime_api_sub_agent_manager(
    workspace: &std::path::Path,
    workers: usize,
) -> crate::tools::subagent::SharedSubAgentManager {
    let max_agents = workers.max(1);
    crate::tools::subagent::new_shared_subagent_manager_with_timeout(
        workspace.to_path_buf(),
        max_agents,
        max_agents,
        std::time::Duration::from_secs(crate::config::DEFAULT_SUBAGENT_HEARTBEAT_TIMEOUT_SECS),
        max_agents,
        None,
    )
}

pub(crate) fn resolve_thread_filter(
    include_archived: Option<bool>,
    archived_only: Option<bool>,
) -> crate::runtime_threads::requests::ThreadListFilter {
    use crate::runtime_threads::requests::ThreadListFilter;
    if archived_only.unwrap_or(false) {
        ThreadListFilter::ArchivedOnly
    } else if include_archived.unwrap_or(false) {
        ThreadListFilter::IncludeArchived
    } else {
        ThreadListFilter::ActiveOnly
    }
}

pub(crate) fn truncate_text(text: &str, max_chars: usize) -> String {
    let char_count = text.chars().count();
    if char_count <= max_chars {
        return text.to_string();
    }
    let truncated: String = text.chars().take(max_chars.saturating_sub(3)).collect();
    format!("{truncated}...")
}

pub(crate) fn parse_iso8601(
    raw: &str,
    field: &str,
) -> Result<chrono::DateTime<chrono::Utc>, ApiError> {
    chrono::DateTime::parse_from_rfc3339(raw)
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .map_err(|e| ApiError::bad_request(format!("Invalid {field} (expected RFC 3339): {e}")))
}

// ── Error mapping helpers ───────────────────────────────────────────

pub(crate) fn map_thread_err(err: anyhow::Error) -> ApiError {
    let message = err.to_string();
    if message.contains("not found") {
        ApiError::not_found(message)
    } else if message.contains("already has an active turn")
        || message.contains("No active turn")
        || message.contains("is not active")
    {
        ApiError {
            status: StatusCode::CONFLICT,
            message,
        }
    } else {
        ApiError::bad_request(message)
    }
}

pub(crate) fn map_task_err(err: anyhow::Error) -> ApiError {
    let message = err.to_string();
    if message.contains("not found") {
        ApiError::not_found(message)
    } else {
        ApiError::bad_request(message)
    }
}

pub(crate) fn map_automation_err(err: anyhow::Error) -> ApiError {
    let message = err.to_string();
    if message.contains("Failed to read automation")
        || message.contains("No such file or directory")
    {
        ApiError::not_found(message)
    } else {
        ApiError::bad_request(message)
    }
}
