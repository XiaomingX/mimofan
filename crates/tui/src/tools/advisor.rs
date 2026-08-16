//! Advisor strategy (#847).
//!
//! Decides WHEN to consult a "frontier" (more capable / more expensive) model
//! vs executing with the default model. The advisor does NOT call any model —
//! it only returns a routing decision ([`ModelChoice`]) that the engine can
//! act on later. Routing is driven by simple, deterministic heuristics:
//!
//! - an explicit user flag (`escalate: true`),
//! - complexity signal keywords in the task (e.g. "design", "architect",
//!   "security review"),
//! - estimated token cost exceeding a configured threshold.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;

use crate::tools::spec::{ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec};

/// The routing decision returned by [`Advisor::advise`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModelChoice {
    /// Execute the task with the default (cheaper) model.
    Execute,
    /// Escalate to a frontier model (name carried in the payload).
    Escalate(FrontierModel),
}

/// Identifier for a frontier model the advisor may escalate to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrontierModel(pub String);

impl FrontierModel {
    #[must_use]
    pub fn name(&self) -> &str {
        &self.0
    }
}

/// Profile of the task the advisor routes on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskProfile {
    /// Free-text task description (scanned for complexity signals).
    pub description: String,
    /// Estimated token cost of executing the task with the default model.
    pub estimated_tokens: u64,
    /// Explicit user override: when true, always escalate regardless of heuristics.
    #[serde(default)]
    pub escalate: bool,
    /// Optional explicit frontier model the user wants; when set and
    /// `escalate` is true, this name is used instead of the default frontier.
    #[serde(default)]
    pub frontier_model: Option<String>,
}

/// Configuration for the advisor heuristics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdvisorConfig {
    /// Default frontier model used when escalating without an explicit name.
    pub default_frontier: String,
    /// Estimated-token ceiling above which the task escalates.
    pub token_threshold: u64,
    /// Lowercased keywords that signal a complex task worth escalating.
    pub complexity_keywords: Vec<String>,
}

impl Default for AdvisorConfig {
    fn default() -> Self {
        Self {
            default_frontier: "frontier".to_string(),
            token_threshold: 32_000,
            complexity_keywords: vec![
                "design".to_string(),
                "architect".to_string(),
                "architecture".to_string(),
                "security review".to_string(),
                "security audit".to_string(),
                "threat model".to_string(),
                "formal verification".to_string(),
                "prove".to_string(),
            ],
        }
    }
}

/// Errors from the advisor.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum AdvisorError {
    /// The task profile was empty / unusable.
    #[error("task profile is empty")]
    EmptyTask,
}

/// The advisor: returns a routing decision without invoking any model.
pub struct Advisor {
    config: AdvisorConfig,
}

impl Default for Advisor {
    fn default() -> Self {
        Self::new(AdvisorConfig::default())
    }
}

impl Advisor {
    /// Build an advisor with explicit config.
    #[must_use]
    pub fn new(config: AdvisorConfig) -> Self {
        Self { config }
    }

    /// Decide whether to execute locally or escalate to a frontier model.
    pub fn advise(&self, task: &TaskProfile) -> Result<ModelChoice, AdvisorError> {
        if task.description.trim().is_empty() {
            return Err(AdvisorError::EmptyTask);
        }

        // 1) Explicit user flag wins.
        if task.escalate {
            return Ok(self.escalate_to(task.frontier_model.clone()));
        }

        // 2) Complexity signal keywords.
        let lowered = task.description.to_lowercase();
        if self
            .config
            .complexity_keywords
            .iter()
            .any(|kw| lowered.contains(kw.as_str()))
        {
            return Ok(self.escalate_to(task.frontier_model.clone()));
        }

        // 3) Estimated token cost ceiling.
        if task.estimated_tokens > self.config.token_threshold {
            return Ok(self.escalate_to(task.frontier_model.clone()));
        }

        Ok(ModelChoice::Execute)
    }

    fn escalate_to(&self, explicit: Option<String>) -> ModelChoice {
        let name = explicit.unwrap_or_else(|| self.config.default_frontier.clone());
        ModelChoice::Escalate(FrontierModel(name))
    }
}

/// Tool wrapper exposing the routing decision to the agent. ReadOnly.
///
/// The advisor never calls a model — it returns a deterministic routing
/// decision ([`ModelChoice`]) that the engine can act on later. Exposing it as
/// a tool lets the model self-route (e.g. "should I escalate this to a
/// frontier model?") without any extra capability.
pub struct AdvisorTool {
    advisor: Advisor,
}

impl Default for AdvisorTool {
    fn default() -> Self {
        Self::new()
    }
}

impl AdvisorTool {
    /// Build the tool with the default advisor config.
    #[must_use]
    pub fn new() -> Self {
        Self {
            advisor: Advisor::default(),
        }
    }
}

#[async_trait]
impl ToolSpec for AdvisorTool {
    fn name(&self) -> &str {
        "advisor"
    }

    fn description(&self) -> &str {
        "Decide whether a task should execute with the default model or escalate to a frontier model. \
         Returns a routing decision (execute / escalate) driven by deterministic heuristics: an \
         explicit escalate flag, complexity-signal keywords, or estimated token cost. Does not call any model."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "description": {
                    "type": "string",
                    "description": "Free-text task description, scanned for complexity signals."
                },
                "estimated_tokens": {
                    "type": "integer",
                    "description": "Estimated token cost of executing the task with the default model."
                },
                "escalate": {
                    "type": "boolean",
                    "description": "Explicit user override: when true, always escalate regardless of heuristics."
                },
                "frontier_model": {
                    "type": "string",
                    "description": "Optional explicit frontier model name to escalate to when escalating."
                }
            },
            "required": ["description"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    async fn execute(&self, input: Value, _context: &ToolContext) -> Result<ToolResult, ToolError> {
        use mimofan_tools::{required_str, optional_str, optional_u64};

        let description = required_str(&input, "description")
            .map_err(|_| ToolError::missing_field("description"))?
            .to_string();
        let estimated_tokens = optional_u64(&input, "estimated_tokens", 0);
        let escalate = input
            .get("escalate")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let frontier_model = optional_str(&input, "frontier_model").map(str::to_string);

        let profile = TaskProfile {
            description,
            estimated_tokens,
            escalate,
            frontier_model,
        };

        let choice = self
            .advisor
            .advise(&profile)
            .map_err(|e| match e {
                AdvisorError::EmptyTask => ToolError::invalid_input("task description is empty"),
            })?;

        let decision = serde_json::to_value(&choice)
            .map_err(|e| ToolError::execution_failed(format!("failed to serialize decision: {e}")))?;

        Ok(ToolResult::success(decision.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(desc: &str, tokens: u64) -> TaskProfile {
        TaskProfile {
            description: desc.to_string(),
            estimated_tokens: tokens,
            escalate: false,
            frontier_model: None,
        }
    }

    #[test]
    fn simple_task_executes() {
        let advisor = Advisor::default();
        let choice = advisor
            .advise(&profile("fix the typo in the README", 120))
            .expect("advise");
        assert_eq!(choice, ModelChoice::Execute);
    }

    #[test]
    fn security_review_escalates() {
        let advisor = Advisor::default();
        let choice = advisor
            .advise(&profile("perform a security review of the auth module", 800))
            .expect("advise");
        match choice {
            ModelChoice::Escalate(f) => assert_eq!(f.name(), "frontier"),
            other => panic!("expected escalate, got {other:?}"),
        }
    }

    #[test]
    fn design_task_escalates() {
        let advisor = Advisor::default();
        let choice = advisor
            .advise(&profile("design the caching layer architecture", 4_000))
            .expect("advise");
        assert!(matches!(choice, ModelChoice::Escalate(_)));
    }

    #[test]
    fn high_token_cost_escalates() {
        let advisor = Advisor::default();
        let choice = advisor
            .advise(&profile("summarize this log file", 64_000))
            .expect("advise");
        assert!(matches!(choice, ModelChoice::Escalate(_)));
    }

    #[test]
    fn explicit_flag_escalates_with_named_model() {
        let advisor = Advisor::default();
        let mut task = profile("trivial rename", 10);
        task.escalate = true;
        task.frontier_model = Some("gpt-frontier".to_string());
        let choice = advisor.advise(&task).expect("advise");
        match choice {
            ModelChoice::Escalate(f) => assert_eq!(f.name(), "gpt-frontier"),
            other => panic!("expected named escalate, got {other:?}"),
        }
    }

    #[test]
    fn empty_task_errors() {
        let advisor = Advisor::default();
        let res = advisor.advise(&profile("", 0));
        assert_eq!(res, Err(AdvisorError::EmptyTask));
    }

    #[test]
    fn below_threshold_no_escalate() {
        let cfg = AdvisorConfig {
            token_threshold: 100,
            ..AdvisorConfig::default()
        };
        let advisor = Advisor::new(cfg);
        let choice = advisor
            .advise(&profile("add a unit test", 99))
            .expect("advise");
        assert_eq!(choice, ModelChoice::Execute);
    }
}
