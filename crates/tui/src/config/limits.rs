//! Unified `[limits]` configuration region.
//!
//! Single source of truth for concurrency / parallelism / timeout / queue
//! knobs that were previously scattered as `const` literals across engine,
//! runtime, task-manager, rlm, and streaming modules. Centralizing them here
//! lets operators tune behavior from `config.toml` / `settings.toml` without
//! recompiling, and keeps the hard ceilings (the `MAX_*` constants) beside the
//! user-overridable defaults they clamp.

use serde::Deserialize;

/// Default maximum number of concurrent sub-agents (used when `[limits]
/// max_subagents` is unset).
pub const DEFAULT_MAX_SUBAGENTS: usize = 8;
/// Hard ceiling for `max_subagents` (and anything that resolves to it). The
/// clamp in every resolver uses this, so no config can push concurrency past
/// it without recompiling. Raised from 20 to 60 to allow large fan-outs on
/// high-core machines.
pub const MAX_SUBAGENTS: usize = 60;
/// Upper bound for queued + running sub-agent admissions. Deliberately higher
/// than the instantaneous concurrency cap so Workflow-style fanout can opt
/// into large bounded populations without unbounded queue growth.
pub const MAX_SUBAGENT_ADMISSION: usize = 200;

/// Default per-step API timeout for sub-agent requests, in seconds.
pub const DEFAULT_SUBAGENT_API_TIMEOUT_SECS: u64 = 120;
/// Minimum accepted `[limits] api_timeout_secs`.
pub const MIN_SUBAGENT_API_TIMEOUT_SECS: u64 = 1;
/// Maximum accepted `[limits] api_timeout_secs` (30 minutes).
pub const MAX_SUBAGENT_API_TIMEOUT_SECS: u64 = 1800;
/// Default wall-clock interval without manager-visible progress before a
/// running child is auto-cancelled to release its slot.
pub const DEFAULT_SUBAGENT_HEARTBEAT_TIMEOUT_SECS: u64 = 300;
/// Minimum accepted `[limits] heartbeat_timeout_secs`.
pub const MIN_SUBAGENT_HEARTBEAT_TIMEOUT_SECS: u64 = 30;
/// Maximum accepted `[limits] heartbeat_timeout_secs` (1 hour).
pub const MAX_SUBAGENT_HEARTBEAT_TIMEOUT_SECS: u64 = 3600;
/// Default per-SSE-chunk idle timeout, in seconds.
pub const DEFAULT_STREAM_CHUNK_TIMEOUT_SECS: u64 = 300;
/// Minimum accepted `[limits] stream_chunk_timeout_secs`.
pub const MIN_STREAM_CHUNK_TIMEOUT_SECS: u64 = 1;
/// Maximum accepted `[limits] stream_chunk_timeout_secs`.
pub const MAX_STREAM_CHUNK_TIMEOUT_SECS: u64 = 3600;

/// Hard ceiling on parallel shell executions within a single engine turn.
pub const MAX_PARALLEL_SHELL_EXEC: usize = 4;
/// Default number of active runtime threads.
pub const MAX_ACTIVE_THREADS_DEFAULT: usize = 8;
/// Default number of task-manager workers.
pub const MAX_TASK_WORKERS: usize = 8;
/// Default max batch size for the RLM bridge.
pub const MAX_RLM_BATCH: usize = 16;

/// Maximum streamed content size, in bytes (10 MB).
pub const STREAM_MAX_CONTENT_BYTES: usize = 10 * 1024 * 1024;
/// Maximum stream duration, in seconds (30 minutes).
pub const STREAM_MAX_DURATION_SECS: u64 = 1800;
/// Maximum transient stream errors before a stream is considered failed.
pub const MAX_STREAM_ERRORS_BEFORE_FAIL: u32 = 5;
/// Maximum transparent stream retries.
pub const MAX_TRANSPARENT_STREAM_RETRIES: u32 = 2;
/// Maximum stream retries.
pub const MAX_STREAM_RETRIES: u32 = 3;

/// `[limits]` configuration region. Every field is optional and falls back to
/// the corresponding `DEFAULT_*` constant above; resolver methods apply the
/// `MIN`/`MAX` clamps so user input can never violate a hard ceiling.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LimitsConfig {
    /// Maximum concurrent sub-agents. Clamped to `[1, MAX_SUBAGENTS]`.
    #[serde(default)]
    pub max_subagents: Option<usize>,
    /// Maximum queued + running sub-agents admitted for a session. Clamped to
    /// `[max_subagents, MAX_SUBAGENT_ADMISSION]`.
    #[serde(default)]
    pub max_admitted_subagents: Option<usize>,
    /// Direct (depth-1) sub-agents that may launch concurrently before the
    /// rest queue for a launch slot. Defaults to the resolved `max_subagents`.
    #[serde(default)]
    pub launch_concurrency: Option<usize>,
    /// Parallel shell executions within a single engine turn. Clamped to
    /// `[1, MAX_PARALLEL_SHELL_EXEC]`.
    #[serde(default)]
    pub max_parallel_shell_exec: Option<usize>,
    /// Number of active runtime threads.
    #[serde(default)]
    pub max_active_threads: Option<usize>,
    /// Number of task-manager workers.
    #[serde(default)]
    pub max_task_workers: Option<usize>,
    /// Max batch size for the RLM bridge.
    #[serde(default)]
    pub max_rlm_batch: Option<usize>,
    /// Per-step sub-agent API timeout, in seconds. Clamped to
    /// `[MIN_SUBAGENT_API_TIMEOUT_SECS, MAX_SUBAGENT_API_TIMEOUT_SECS]`.
    #[serde(default)]
    pub api_timeout_secs: Option<u64>,
    /// Wall-clock heartbeat timeout for a stalled running child, in seconds.
    /// Clamped to `[MIN_SUBAGENT_HEARTBEAT_TIMEOUT_SECS,
    /// MAX_SUBAGENT_HEARTBEAT_TIMEOUT_SECS]`.
    #[serde(default)]
    pub heartbeat_timeout_secs: Option<u64>,
    /// Per-SSE-chunk idle timeout, in seconds. Clamped to
    /// `[MIN_STREAM_CHUNK_TIMEOUT_SECS, MAX_STREAM_CHUNK_TIMEOUT_SECS]`.
    #[serde(default)]
    pub stream_chunk_timeout_secs: Option<u64>,
    /// Maximum streamed content size, in bytes.
    #[serde(default)]
    pub stream_max_content_bytes: Option<usize>,
    /// Maximum stream duration, in seconds.
    #[serde(default)]
    pub stream_max_duration_secs: Option<u64>,
    /// Maximum transient stream errors before failure.
    #[serde(default)]
    pub max_stream_errors_before_fail: Option<u32>,
    /// Maximum transparent stream retries.
    #[serde(default)]
    pub max_transparent_stream_retries: Option<u32>,
    /// Maximum stream retries.
    #[serde(default)]
    pub max_stream_retries: Option<u32>,
}
