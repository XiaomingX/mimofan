//! Sub-agent runtime.
//!
//! Runtime management for sub-agent execution.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;

use super::events::SubAgentEvent;
use super::types::{SubAgentCompletion, SubAgentStatus};

/// Runtime configuration for sub-agents.
#[derive(Debug, Clone)]
pub struct SubAgentRuntimeConfig {
    /// Maximum steps for sub-agent loops.
    pub max_steps: u32,
    /// Wall-clock budget for a single sub-agent tool execution.
    pub tool_timeout: Duration,
    /// Maximum number of concurrent sub-agents.
    pub max_concurrent: usize,
    /// Whether to enable streaming.
    pub enable_streaming: bool,
}

impl Default for SubAgentRuntimeConfig {
    fn default() -> Self {
        Self {
            max_steps: u32::MAX,
            tool_timeout: Duration::from_secs(300),
            max_concurrent: 10,
            enable_streaming: true,
        }
    }
}

/// Runtime state for a sub-agent.
#[derive(Debug)]
pub struct SubAgentRuntimeState {
    /// Current status.
    pub status: SubAgentStatus,
    /// Start time.
    pub started_at: Instant,
    /// Steps taken.
    pub steps_taken: u32,
    /// Current step.
    pub current_step: u32,
    /// Events collected.
    pub events: Vec<SubAgentEvent>,
}

impl Default for SubAgentRuntimeState {
    fn default() -> Self {
        Self::new()
    }
}

impl SubAgentRuntimeState {
    /// Create a new runtime state.
    #[must_use]
    pub fn new() -> Self {
        Self {
            status: SubAgentStatus::Running,
            started_at: Instant::now(),
            steps_taken: 0,
            current_step: 0,
            events: Vec::new(),
        }
    }

    /// Check if the runtime has exceeded its time budget.
    #[must_use]
    pub fn is_timed_out(&self, timeout: Duration) -> bool {
        self.started_at.elapsed() > timeout
    }

    /// Check if the runtime has exceeded its step budget.
    #[must_use]
    pub fn is_step_exceeded(&self, max_steps: u32) -> bool {
        self.current_step >= max_steps
    }

    /// Record an event.
    pub fn record_event(&mut self, event: SubAgentEvent) {
        self.events.push(event);
    }

    /// Get elapsed time.
    #[must_use]
    pub fn elapsed(&self) -> Duration {
        self.started_at.elapsed()
    }
}

/// Runtime handle for a sub-agent.
#[derive(Debug)]
pub struct SubAgentRuntimeHandle {
    /// Agent ID.
    pub agent_id: String,
    /// Join handle for the agent task.
    pub handle: JoinHandle<Result<SubAgentCompletion>>,
    /// Cancellation token.
    pub cancel: tokio_util::sync::CancellationToken,
}

impl SubAgentRuntimeHandle {
    /// Cancel the agent.
    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    /// Check if the agent is finished.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.handle.is_finished()
    }
}

/// Runtime manager for sub-agents.
pub struct SubAgentRuntimeManager {
    /// Active runtime handles.
    handles: Arc<RwLock<Vec<SubAgentRuntimeHandle>>>,
    /// Runtime configuration.
    config: SubAgentRuntimeConfig,
}

impl SubAgentRuntimeManager {
    /// Create a new runtime manager.
    #[must_use]
    pub fn new(config: SubAgentRuntimeConfig) -> Self {
        Self {
            handles: Arc::new(RwLock::new(Vec::new())),
            config,
        }
    }

    /// Get the runtime configuration.
    #[must_use]
    pub fn config(&self) -> &SubAgentRuntimeConfig {
        &self.config
    }

    /// Register a runtime handle.
    pub async fn register(&self, handle: SubAgentRuntimeHandle) {
        let mut handles = self.handles.write().await;
        handles.push(handle);
    }

    /// Unregister a runtime handle.
    pub async fn unregister(&self, agent_id: &str) {
        let mut handles = self.handles.write().await;
        handles.retain(|h| h.agent_id != agent_id);
    }

    /// Get the number of active handles.
    #[must_use]
    pub async fn active_count(&self) -> usize {
        let handles = self.handles.read().await;
        handles.len()
    }

    /// Cancel all active handles.
    pub async fn cancel_all(&self) {
        let handles = self.handles.read().await;
        for handle in handles.iter() {
            handle.cancel();
        }
    }

    /// Get active agent IDs.
    #[must_use]
    pub async fn active_agent_ids(&self) -> Vec<String> {
        let handles = self.handles.read().await;
        handles.iter().map(|h| h.agent_id.clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sub_agent_runtime_config_default() {
        let config = SubAgentRuntimeConfig::default();
        assert_eq!(config.max_steps, u32::MAX);
        assert_eq!(config.tool_timeout, Duration::from_secs(300));
        assert_eq!(config.max_concurrent, 10);
        assert!(config.enable_streaming);
    }

    #[test]
    fn test_sub_agent_runtime_state_new() {
        let state = SubAgentRuntimeState::new();
        assert_eq!(state.status, SubAgentStatus::Running);
        assert_eq!(state.steps_taken, 0);
        assert_eq!(state.current_step, 0);
        assert!(state.events.is_empty());
    }

    #[test]
    fn test_sub_agent_runtime_state_is_timed_out() {
        let mut state = SubAgentRuntimeState::new();
        state.started_at = Instant::now() - Duration::from_secs(10);
        assert!(state.is_timed_out(Duration::from_secs(5)));
        assert!(!state.is_timed_out(Duration::from_secs(15)));
    }

    #[test]
    fn test_sub_agent_runtime_state_is_step_exceeded() {
        let mut state = SubAgentRuntimeState::new();
        state.current_step = 10;
        assert!(state.is_step_exceeded(5));
        assert!(!state.is_step_exceeded(15));
    }

    #[tokio::test]
    async fn test_sub_agent_runtime_manager() {
        let config = SubAgentRuntimeConfig::default();
        let manager = SubAgentRuntimeManager::new(config);
        assert_eq!(manager.active_count().await, 0);
        assert!(manager.active_agent_ids().await.is_empty());
    }
}
