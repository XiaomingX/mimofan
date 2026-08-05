//! Sub-agent type definitions.
//!
//! Core types for sub-agent orchestration, status, and results.

use std::collections::VecDeque;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::models::{Message, SystemPrompt};

use super::default_agent_inspect_tool;
use super::default_agent_run_follow_up;
use super::default_agent_run_recommended_action;
use super::default_agent_run_takeover;
use super::default_agent_run_usage;
use super::default_agent_run_verification;
use super::default_subagent_actor_kind;

use crate::worker_profile::WorkerRuntimeProfile;

/// Assignment metadata for sub-agent orchestration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SubAgentAssignment {
    pub objective: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

impl SubAgentAssignment {
    pub fn new(objective: String, role: Option<String>) -> Self {
        Self { objective, role }
    }
}

/// Sub-agent execution types with specialized behavior and tool access.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SubAgentType {
    /// General purpose - full tool access for multi-step tasks.
    #[default]
    General,
    /// Fast exploration - read-only tools for codebase search.
    Explore,
    /// Planning - analysis tools only for architectural planning.
    Plan,
    /// Code review - read + analysis tools.
    Review,
    /// Implementation — focused on writing / patching code to satisfy
    /// a specific change.
    Implementer,
    /// Verification — focused on running the test suite or other
    /// validation gates and reporting pass/fail with evidence.
    Verifier,
    /// Custom tool access defined at spawn time.
    Custom,
}

impl SubAgentType {
    /// Parse a sub-agent type from user input.
    #[must_use]
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "general" | "general-purpose" | "general_purpose" | "worker" | "default" => {
                Some(Self::General)
            }
            "explore" | "exploration" | "explorer" => Some(Self::Explore),
            "plan" | "planning" | "planner" | "awaiter" => Some(Self::Plan),
            "review" | "code-review" | "code_review" | "reviewer" => Some(Self::Review),
            "implementer" | "implement" | "implementation" | "builder" => Some(Self::Implementer),
            "verifier" | "verify" | "verification" | "validator" | "tester" => Some(Self::Verifier),
            "custom" => Some(Self::Custom),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::General => "general",
            Self::Explore => "explore",
            Self::Plan => "plan",
            Self::Review => "review",
            Self::Implementer => "implementer",
            Self::Verifier => "verifier",
            Self::Custom => "custom",
        }
    }

    /// Get the system prompt for this agent type.
    #[must_use]
    pub fn system_prompt(&self) -> String {
        format!("You are a {} sub-agent.", self.as_str())
    }
}

/// Status of a sub-agent execution.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SubAgentStatus {
    Running,
    Completed,
    Interrupted(String),
    Failed(String),
    Cancelled,
    /// Worker stopped because it exceeded its own per-worker token budget.
    BudgetExhausted,
}

/// Structured reason a non-running sub-agent needs parent action.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SubAgentNeedsInput {
    pub question: String,
}

/// Snapshot of sub-agent state for tool results.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentResult {
    pub name: String,
    pub agent_id: String,
    pub context_mode: String,
    pub fork_context: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
    pub agent_type: SubAgentType,
    pub assignment: SubAgentAssignment,
    #[serde(default)]
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nickname: Option<String>,
    pub status: SubAgentStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_status: Option<AgentWorkerStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_run_id: Option<String>,
    #[serde(default)]
    pub spawn_depth: u32,
    pub result: Option<String>,
    pub steps_taken: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkpoint: Option<SubAgentCheckpoint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub needs_input: Option<SubAgentNeedsInput>,
    pub duration_ms: u64,
    #[serde(default, skip_serializing_if = "is_false")]
    pub from_prior_session: bool,
}

/// Headless worker lifecycle states for sub-agent execution.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AgentWorkerStatus {
    Queued,
    Starting,
    Running,
    WaitingForUser,
    ModelWait,
    RunningTool,
    Completed,
    Failed,
    Cancelled,
    Interrupted,
}

/// Tool profile for agent worker.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AgentWorkerToolProfile {
    Inherited,
    Explicit(Vec<String>),
}

/// Specification for spawning an agent worker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentWorkerSpec {
    pub worker_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub run_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_name: Option<String>,
    pub objective: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    pub agent_type: SubAgentType,
    pub model: String,
    pub workspace: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
    pub context_mode: String,
    pub fork_context: bool,
    pub tool_profile: AgentWorkerToolProfile,
    #[serde(default)]
    pub runtime_profile: WorkerRuntimeProfile,
    pub max_steps: u32,
    pub spawn_depth: u32,
    pub max_spawn_depth: u32,
}

/// Follow-up delivery target for agent runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRunFollowUpDelivery {
    pub delivered: bool,
    pub timestamp_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_preview: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub interrupt: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub continued_from_checkpoint: bool,
}

/// Follow-up target for agent runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRunFollowUpTarget {
    #[serde(default = "default_agent_inspect_tool")]
    pub tool: String,
    pub agent_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_name: Option<String>,
    #[serde(default)]
    pub accepted_statuses: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_delivery: Option<AgentRunFollowUpDelivery>,
}

/// Takeover target for agent runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRunTakeoverTarget {
    #[serde(default = "default_subagent_takeover_kind")]
    pub kind: String,
    #[serde(default)]
    pub supported: bool,
    pub agent_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_name: Option<String>,
    pub instructions: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unsupported_reason: Option<String>,
}

/// Artifact reference for agent runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRunArtifactRef {
    pub kind: String,
    pub name: String,
    pub target: String,
    #[serde(default)]
    pub description: String,
}

/// Usage statistics for agent runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRunUsage {
    #[serde(default = "default_usage_status")]
    pub status: String,
    #[serde(default)]
    pub input_tokens: Option<u64>,
    #[serde(default)]
    pub output_tokens: Option<u64>,
    #[serde(default)]
    pub total_tokens: Option<u64>,
    #[serde(default)]
    pub token_budget: Option<u64>,
    #[serde(default)]
    pub budget_spent_tokens: Option<u64>,
    #[serde(default)]
    pub budget_remaining_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_scope: Option<String>,
    #[serde(default = "default_usage_note")]
    pub note: String,
}

/// Verification summary for agent runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRunVerificationSummary {
    pub status: String,
    pub summary: String,
}

/// Recommended action for agent runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRunRecommendedAction {
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    pub reason: String,
}

/// Worker event for agent runs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentWorkerEvent {
    pub seq: u64,
    pub worker_id: String,
    pub status: AgentWorkerStatus,
    pub timestamp_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
}

/// Worker record for agent runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentWorkerRecord {
    pub spec: AgentWorkerSpec,
    #[serde(default = "default_subagent_actor_kind")]
    pub actor_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_run_id: Option<String>,
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
    #[serde(default = "default_agent_run_recommended_action")]
    pub recommended_action: AgentRunRecommendedAction,
    pub status: AgentWorkerStatus,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default)]
    pub steps_taken: u32,
    #[serde(default)]
    pub events: VecDeque<AgentWorkerEvent>,
}

impl AgentWorkerRecord {
    pub fn new(spec: AgentWorkerSpec, now_ms: u64) -> Self {
        Self {
            spec,
            actor_kind: default_subagent_actor_kind(),
            parent_run_id: None,
            follow_up: default_agent_run_follow_up(),
            takeover: default_agent_run_takeover(),
            artifacts: Vec::new(),
            usage: default_agent_run_usage(),
            verification: default_agent_run_verification(),
            recommended_action: default_agent_run_recommended_action(),
            status: AgentWorkerStatus::Queued,
            created_at_ms: now_ms,
            updated_at_ms: now_ms,
            started_at_ms: None,
            completed_at_ms: None,
            latest_message: None,
            result_summary: None,
            error: None,
            steps_taken: 0,
            events: VecDeque::new(),
        }
    }

    pub fn update_status(&mut self, status: AgentWorkerStatus) {
        self.status = status;
        self.updated_at_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
    }
}

/// Model strength for sub-agents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SubAgentModelStrength {
    Strong,
    Medium,
    Weak,
}

/// Thinking mode for sub-agents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SubAgentThinking {
    Enabled,
    Disabled,
    Auto,
}

/// Checkpoint for sub-agent state.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SubAgentCheckpoint {
    pub checkpoint_id: String,
    pub agent_id: String,
    pub continuation_handle: String,
    pub reason: String,
    pub continuable: bool,
    pub steps_taken: u32,
    pub message_count: usize,
    pub created_at_ms: u64,
    pub messages: Vec<Message>,
}

/// Error type for sub-agent operations.
#[derive(Debug, thiserror::Error)]
pub enum SubAgentError {
    #[error("Agent not found: {0}")]
    NotFound(String),

    #[error("Agent already exists: {0}")]
    AlreadyExists(String),

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("Timeout after {0}ms")]
    Timeout(u64),

    #[error("Agent failed: {0}")]
    Failed(String),

    #[error("Agent cancelled")]
    Cancelled,

    #[error("Agent interrupted")]
    Interrupted,

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Other error: {0}")]
    Other(String),
}

impl SubAgentError {
    pub fn not_found(id: impl Into<String>) -> Self {
        Self::NotFound(id.into())
    }

    pub fn already_exists(id: impl Into<String>) -> Self {
        Self::AlreadyExists(id.into())
    }

    pub fn invalid_config(msg: impl Into<String>) -> Self {
        Self::InvalidConfig(msg.into())
    }

    pub fn timeout(ms: u64) -> Self {
        Self::Timeout(ms)
    }

    pub fn failed(msg: impl Into<String>) -> Self {
        Self::Failed(msg.into())
    }

    pub fn other(msg: impl Into<String>) -> Self {
        Self::Other(msg.into())
    }
}

impl From<SubAgentError> for String {
    fn from(err: SubAgentError) -> String {
        err.to_string()
    }
}

/// Default functions for serde
fn default_usage_status() -> String {
    "unknown".to_string()
}

fn default_usage_note() -> String {
    "Token usage is not yet reported by the sub-agent worker ledger.".to_string()
}

fn default_subagent_takeover_kind() -> String {
    "sub_agent".to_string()
}

/// Persisted sub-agent state for a single agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedSubAgent {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_name: Option<String>,
    #[serde(default)]
    pub fork_context: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<PathBuf>,
    pub agent_type: SubAgentType,
    pub prompt: String,
    pub assignment: SubAgentAssignment,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub nickname: Option<String>,
    pub status: SubAgentStatus,
    pub result: Option<String>,
    pub steps_taken: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkpoint: Option<SubAgentCheckpoint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub needs_input: Option<SubAgentNeedsInput>,
    pub duration_ms: u64,
    pub allowed_tools: Vec<String>,
    pub updated_at_ms: u64,
    #[serde(default)]
    pub session_boot_id: String,
}

/// Persisted sub-agent state for all agents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedSubAgentState {
    pub schema_version: u32,
    pub agents: Vec<PersistedSubAgent>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workers: Vec<AgentWorkerRecord>,
}

impl Default for PersistedSubAgentState {
    fn default() -> Self {
        Self {
            schema_version: 1,
            agents: Vec::new(),
            workers: Vec::new(),
        }
    }
}

/// Completion result for sub-agents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentCompletion {
    pub agent_id: String,
    pub payload: String,
}

/// Fork context for sub-agents.
#[derive(Debug, Clone)]
pub struct SubAgentForkContext {
    pub system: Option<SystemPrompt>,
    pub messages: Vec<Message>,
    pub structured_state_block: Option<String>,
}

/// Helper function to check if a boolean is false.
pub fn is_false(b: &bool) -> bool {
    !*b
}

/// Default cap on sub-agent recursion depth. Override via
/// `[subagents] max_depth = N` in config.
pub const DEFAULT_MAX_SPAWN_DEPTH: u32 = mimofan_config::DEFAULT_SPAWN_DEPTH;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sub_agent_type_from_str() {
        assert_eq!(
            SubAgentType::from_str("general"),
            Some(SubAgentType::General)
        );
        assert_eq!(
            SubAgentType::from_str("explore"),
            Some(SubAgentType::Explore)
        );
        assert_eq!(SubAgentType::from_str("plan"), Some(SubAgentType::Plan));
        assert_eq!(SubAgentType::from_str("review"), Some(SubAgentType::Review));
        assert_eq!(
            SubAgentType::from_str("implementer"),
            Some(SubAgentType::Implementer)
        );
        assert_eq!(
            SubAgentType::from_str("verifier"),
            Some(SubAgentType::Verifier)
        );
        assert_eq!(SubAgentType::from_str("custom"), Some(SubAgentType::Custom));
        assert_eq!(SubAgentType::from_str("unknown"), None);
    }

    #[test]
    fn test_sub_agent_type_as_str() {
        assert_eq!(SubAgentType::General.as_str(), "general");
        assert_eq!(SubAgentType::Explore.as_str(), "explore");
        assert_eq!(SubAgentType::Plan.as_str(), "plan");
        assert_eq!(SubAgentType::Review.as_str(), "review");
        assert_eq!(SubAgentType::Implementer.as_str(), "implementer");
        assert_eq!(SubAgentType::Verifier.as_str(), "verifier");
        assert_eq!(SubAgentType::Custom.as_str(), "custom");
    }

    #[test]
    fn test_agent_worker_record_new() {
        let spec = AgentWorkerSpec {
            worker_id: "agent-123".to_string(),
            run_id: String::new(),
            parent_run_id: None,
            session_name: None,
            objective: "test task".to_string(),
            role: None,
            agent_type: SubAgentType::General,
            model: "gpt-4".to_string(),
            workspace: PathBuf::from("/tmp"),
            git_branch: None,
            context_mode: "default".to_string(),
            fork_context: false,
            tool_profile: AgentWorkerToolProfile::Inherited,
            runtime_profile: WorkerRuntimeProfile::default(),
            max_steps: 100,
            spawn_depth: 0,
            max_spawn_depth: 6,
        };
        let record = AgentWorkerRecord::new(spec, 0);
        assert_eq!(record.spec.worker_id, "agent-123");
        assert_eq!(record.spec.agent_type, SubAgentType::General);
        assert_eq!(record.status, AgentWorkerStatus::Queued);
    }

    #[test]
    fn test_persisted_sub_agent_state_default() {
        let state = PersistedSubAgentState::default();
        assert_eq!(state.schema_version, 1);
        assert!(state.agents.is_empty());
        assert!(state.workers.is_empty());
    }
}
