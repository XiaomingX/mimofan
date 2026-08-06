//! Tools configuration and status items.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::provider::ApiProvider;

/// Model-visible tool catalog controls (`[tools]` table in config.toml).
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ToolsConfig {
    /// Native tool names to keep loaded even when they are outside the small
    /// default core catalog. Unknown names are harmless and simply never match.
    #[serde(default)]
    pub always_load: Vec<String>,

    /// Optional directory to scan for plugin tool scripts.
    #[serde(default)]
    pub plugin_dir: Option<String>,

    /// Per-tool overrides keyed by built-in tool name.
    #[serde(default)]
    pub overrides: Option<HashMap<String, ToolOverride>>,
}

/// One configurable footer item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
pub enum StatusItem {
    /// "agent" / "yolo" / "plan" chip.
    Mode,
    /// Model identifier (e.g. `deepseek-v4-pro`).
    Model,
    /// Session cost in the configured display currency.
    Cost,
    /// Activity label: "idle" / "busy" / "draft" / "working".
    Status,
    /// Sub-agent count chip ("3 agents").
    Agents,
    /// Reasoning-replay token count ("rsn 12.3k").
    ReasoningReplay,
    /// Prefix stability ("cache prefix 100%").
    PrefixStability,
    /// Cache hit rate ("cache 73%").
    Cache,
    /// Context-window utilisation percent ("48%").
    ContextPercent,
    /// Current git branch name.
    GitBranch,
    /// Elapsed time of the most recent tool call (placeholder until wired).
    LastToolElapsed,
    /// Remaining rate-limit budget (placeholder until wired).
    RateLimit,
    /// Session token usage: input / cache-hit / output.
    Tokens,
    /// DeepSeek account balance, refreshed once per turn completion.
    Balance,
    /// Output token throughput (TPS) and time-to-first-token (TTFT).
    Throughput,
}

impl StatusItem {
    /// Default footer composition for the always-on status line.
    #[must_use]
    pub fn default_footer() -> Vec<StatusItem> {
        vec![
            StatusItem::Mode,
            StatusItem::Model,
            StatusItem::Cost,
            StatusItem::Status,
            StatusItem::Agents,
            StatusItem::ReasoningReplay,
            StatusItem::Cache,
            StatusItem::GitBranch,
            StatusItem::Tokens,
            StatusItem::Throughput,
        ]
    }

    /// Stable canonical name used in TOML and the picker label.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            StatusItem::Mode => "mode",
            StatusItem::Model => "model",
            StatusItem::Cost => "cost",
            StatusItem::Status => "status",
            StatusItem::Agents => "agents",
            StatusItem::ReasoningReplay => "reasoning_replay",
            StatusItem::PrefixStability => "prefix_stability",
            StatusItem::Cache => "cache",
            StatusItem::ContextPercent => "context_percent",
            StatusItem::GitBranch => "git_branch",
            StatusItem::LastToolElapsed => "last_tool_elapsed",
            StatusItem::RateLimit => "rate_limit",
            StatusItem::Tokens => "tokens",
            StatusItem::Balance => "balance",
            StatusItem::Throughput => "throughput",
        }
    }

    /// Reverse of [`key`](Self::key): parse a config string back to a variant.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "mode" => Some(Self::Mode),
            "model" => Some(Self::Model),
            "cost" => Some(Self::Cost),
            "status" => Some(Self::Status),
            "agents" => Some(Self::Agents),
            "reasoning_replay" => Some(Self::ReasoningReplay),
            "prefix_stability" => Some(Self::PrefixStability),
            "cache" => Some(Self::Cache),
            "context_percent" => Some(Self::ContextPercent),
            "git_branch" => Some(Self::GitBranch),
            "last_tool_elapsed" => Some(Self::LastToolElapsed),
            "rate_limit" => Some(Self::RateLimit),
            "tokens" => Some(Self::Tokens),
            "balance" => Some(Self::Balance),
            "throughput" => Some(Self::Throughput),
            _ => None,
        }
    }

    /// Human-readable label for the picker.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            StatusItem::Mode => "Mode",
            StatusItem::Model => "Model",
            StatusItem::Cost => "Session cost",
            StatusItem::Status => "Activity (idle/busy/draft/working)",
            StatusItem::Agents => "Sub-agents in flight",
            StatusItem::ReasoningReplay => "Reasoning replay tokens",
            StatusItem::PrefixStability => "Prefix stability",
            StatusItem::Cache => "Prompt cache hit rate",
            StatusItem::ContextPercent => "Context window %",
            StatusItem::GitBranch => "Git branch",
            StatusItem::LastToolElapsed => "Last tool elapsed",
            StatusItem::RateLimit => "Rate-limit remaining",
            StatusItem::Tokens => "Session tokens",
            StatusItem::Balance => "Account balance",
            StatusItem::Throughput => "Token throughput & TTFT",
        }
    }

    /// One-line hint shown beside the label.
    #[must_use]
    pub fn hint(self) -> &'static str {
        match self {
            StatusItem::Mode => "agent · yolo · plan",
            StatusItem::Model => "the model id you'll send to",
            StatusItem::Cost => "running total for this session",
            StatusItem::Status => "what the agent is doing right now",
            StatusItem::Agents => "agents or RLM work in progress",
            StatusItem::ReasoningReplay => "thinking tokens replayed each turn",
            StatusItem::PrefixStability => "whether system/tools stayed cacheable",
            StatusItem::Cache => "% of prompt served from cache",
            StatusItem::ContextPercent => "tokens used / model context window",
            StatusItem::GitBranch => "current workspace branch",
            StatusItem::LastToolElapsed => "ms of the most recent tool call (reserved)",
            StatusItem::RateLimit => "remaining requests in the budget (reserved)",
            StatusItem::Tokens => "input / cache-hit / output token totals",
            StatusItem::Balance => "topped-up + granted balance from DeepSeek",
            StatusItem::Throughput => "output tokens/sec and time-to-first-token",
        }
    }

    /// Every variant in display order.
    #[must_use]
    pub fn all() -> &'static [StatusItem] {
        &[
            StatusItem::Mode,
            StatusItem::Model,
            StatusItem::Cost,
            StatusItem::Balance,
            StatusItem::Status,
            StatusItem::Agents,
            StatusItem::ReasoningReplay,
            StatusItem::PrefixStability,
            StatusItem::Cache,
            StatusItem::ContextPercent,
            StatusItem::GitBranch,
            StatusItem::LastToolElapsed,
            StatusItem::RateLimit,
            StatusItem::Tokens,
            StatusItem::Throughput,
        ]
    }

    /// Items that belong in the footer's left cluster (steady identity).
    #[must_use]
    pub fn is_left_cluster(self) -> bool {
        matches!(
            self,
            StatusItem::Mode
                | StatusItem::Model
                | StatusItem::Cost
                | StatusItem::Status
                | StatusItem::Balance
        )
    }

    /// Whether this item is relevant for `provider`.
    #[must_use]
    pub fn is_available_for(self, provider: ApiProvider) -> bool {
        match self {
            StatusItem::Balance => matches!(provider, ApiProvider::OpenAiCompatible),
            StatusItem::RateLimit => {
                matches!(provider, ApiProvider::OpenAiCompatible)
            }
            _ => true,
        }
    }
}

/// Resolved retry policy with defaults applied.
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    pub enabled: bool,
    pub max_retries: u32,
    pub initial_delay: f64,
    pub max_delay: f64,
    pub exponential_base: f64,
}

impl RetryPolicy {
    /// Compute the backoff delay for a retry attempt.
    #[must_use]
    pub fn delay_for_attempt(&self, attempt: u32) -> std::time::Duration {
        let exponent = i32::try_from(attempt).unwrap_or(i32::MAX);
        let delay = self.initial_delay * self.exponential_base.powi(exponent);
        let delay = delay.min(self.max_delay);
        let delay = delay.clamp(0.0, 300.0);
        std::time::Duration::from_secs_f64(delay)
    }
}

/// Tool override configuration.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum ToolOverride {
    /// Run a local script file.
    Script {
        /// Path to the script.
        path: String,
        /// Optional static arguments prepended before the tool's JSON input.
        #[serde(default)]
        args: Option<Vec<String>>,
    },
    /// Run an external command.
    Command {
        /// The command to run.
        command: String,
        /// Optional static arguments prepended before the tool's JSON input.
        #[serde(default)]
        args: Option<Vec<String>>,
    },
    /// Completely disable a built-in tool.
    Disabled,
}

/// Vision model configuration for the `image_analyze` tool.
#[derive(Debug, Clone, Deserialize)]
pub struct VisionModelConfig {
    /// Model identifier (e.g., "gemini-3.1-flash-lite-preview").
    pub model: String,
    /// API key for the vision model.
    #[serde(default)]
    pub api_key: Option<String>,
    /// Base URL for the vision model API.
    #[serde(default)]
    pub base_url: Option<String>,
}
