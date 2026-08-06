//! Sub-agent concurrency/timeout clamps and their resolvers.
//!
//! The numeric constants now live in the unified `limits` module (single
//! source of truth for all concurrency/timeout knobs). This module re-exports
//! the sub-agent-relevant subset so the historical `crate::config::<CONST>`
//! paths keep resolving unchanged, and keeps the two private clamp helpers
//! that operate on them (#3311).

pub use crate::config::limits::{
    DEFAULT_SUBAGENT_API_TIMEOUT_SECS, DEFAULT_SUBAGENT_HEARTBEAT_TIMEOUT_SECS,
    MAX_SUBAGENT_API_TIMEOUT_SECS, MAX_SUBAGENT_HEARTBEAT_TIMEOUT_SECS,
    MIN_SUBAGENT_API_TIMEOUT_SECS, MIN_SUBAGENT_HEARTBEAT_TIMEOUT_SECS,
};
// `MAX_SUBAGENTS` 已统一到 `limits` 模块；此处 re-export 保持
// `crate::config::MAX_SUBAGENTS` / `super::subagent_limits::MAX_SUBAGENTS`
// 历史路径不变（如 env_overrides.rs 的引用），单一真源仍在 limits.rs。
pub use crate::config::limits::MAX_SUBAGENTS;
pub(crate) const STREAM_CHUNK_TIMEOUT_ENV: &str = "MIMOFAN_STREAM_IDLE_TIMEOUT_SECS";

pub(crate) fn resolve_subagent_api_timeout_secs(raw: Option<u64>) -> u64 {
    let raw = raw.unwrap_or(DEFAULT_SUBAGENT_API_TIMEOUT_SECS);
    if raw == 0 {
        return DEFAULT_SUBAGENT_API_TIMEOUT_SECS;
    }
    raw.clamp(MIN_SUBAGENT_API_TIMEOUT_SECS, MAX_SUBAGENT_API_TIMEOUT_SECS)
}

pub(crate) fn resolve_subagent_heartbeat_timeout_secs(
    raw: Option<u64>,
    api_timeout_secs: u64,
) -> u64 {
    let raw = raw.unwrap_or(DEFAULT_SUBAGENT_HEARTBEAT_TIMEOUT_SECS);
    let configured = if raw == 0 {
        DEFAULT_SUBAGENT_HEARTBEAT_TIMEOUT_SECS
    } else {
        raw.clamp(
            MIN_SUBAGENT_HEARTBEAT_TIMEOUT_SECS,
            MAX_SUBAGENT_HEARTBEAT_TIMEOUT_SECS,
        )
    };
    let min_for_api = api_timeout_secs.saturating_add(30).clamp(
        MIN_SUBAGENT_HEARTBEAT_TIMEOUT_SECS,
        MAX_SUBAGENT_HEARTBEAT_TIMEOUT_SECS,
    );
    configured.max(min_for_api)
}
