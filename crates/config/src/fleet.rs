//! Fleet configuration types (#3165).
//!
//! On-disk schema for the `[fleet]` table. See `config.example.toml`
//! and `docs/FLEET.md` for documentation.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Canonical recursion-depth policy for the headless worker runtime.
///
/// Single source of truth shared by BOTH standalone sub-agents and fleet
/// workers so the two cannot drift into "two moving targets":
/// - [`DEFAULT_SPAWN_DEPTH`] is the default recursion budget (the sub-agent
///   runtime's `DEFAULT_MAX_SPAWN_DEPTH` is defined as this value).
/// - [`MAX_SPAWN_DEPTH_CEILING`] is the opt-in safety cap; every configured
///   value (fleet `max_spawn_depth`, the `agent` tool's `max_depth`) clamps to it.
///
/// A worker runs at `spawn_depth = 0` and may spawn while
/// `spawn_depth + 1 <= max_spawn_depth`, so a depth of N affords N nested
/// delegation levels below the root worker. The default of 3 affords at least
/// three recursion levels out of the box; the root worker still runs at
/// depth 0 even when the budget is 0.
pub const DEFAULT_SPAWN_DEPTH: u32 = 3;

/// Hard ceiling on recursion depth for any worker/sub-agent. The default stays
/// conservative at [`DEFAULT_SPAWN_DEPTH`], while explicit config can opt into
/// deeper trees for direct-API providers that can tolerate the fanout.
/// Raising this single constant lifts the limit everywhere (the fleet clamp
/// and `agent` validation both read it).
pub const MAX_SPAWN_DEPTH_CEILING: u32 = 8;

/// On-disk schema for the `[fleet]` table (#3165).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FleetConfigToml {
    /// Default trust level for fleet workers. One of `"sandbox"`, `"local"`,
    /// `"remote-verified"`, or `"operator"`. Defaults to `"sandbox"`.
    #[serde(default = "default_fleet_trust_level_str")]
    pub default_trust_level: String,
    /// Require identity verification for remote (SSH) workers before
    /// granting them `remote-verified` trust. Defaults to true.
    #[serde(default = "default_fleet_require_identity")]
    pub require_identity_verification: bool,
    /// Maximum trust level any worker may have (`"sandbox"`, `"local"`,
    /// `"remote-verified"`, or `"operator"`). Defaults to `"operator"`.
    #[serde(default = "default_fleet_max_trust_level_str")]
    pub max_trust_level: String,
    /// User-defined and built-in role presets.
    ///
    /// Each role defines default tool profiles, capabilities, budgets, and
    /// trust settings that task specs can reference by name. Built-in roles
    /// (`smoke-runner`, `reviewer`, `builder`, `read-only`) are always
    /// available; user-defined roles in config override or extend them.
    #[serde(default)]
    pub roles: BTreeMap<String, FleetRolePreset>,
    /// Fleet profile vocabulary (#3167). Profiles group role semantics,
    /// loadout hints, permission defaults, and delegation bounds.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub profiles: BTreeMap<String, FleetProfile>,
    /// Headless worker execution hardening (#3027).
    #[serde(default)]
    pub exec: FleetExecConfig,
}

/// Headless worker execution constraints (#3027).
///
/// These limits apply to all fleet workers and sub-agents spawned through
/// the headless worker runtime. Task specs can tighten but not loosen them.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FleetExecConfig {
    /// Tools that are always allowed regardless of role or task spec.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_tools: Vec<String>,
    /// Tools that are always disallowed, overriding role and task spec.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub disallowed_tools: Vec<String>,
    /// Hard ceiling on sub-agent steps (tool calls + model turns).
    /// Workers that exceed this are terminated. Default: unbounded (u32::MAX).
    #[serde(default = "default_fleet_max_turns")]
    pub max_turns: u32,
    /// Recursive child-agent budget for headless fleet workers.
    /// Defaults to [`DEFAULT_SPAWN_DEPTH`] (3) so a fleet worker has the SAME
    /// recursion budget as a standalone sub-agent — fleet and sub-agents are one
    /// substrate, not two. Set 0 to block child `agent` calls (the root worker
    /// still runs); the value is clamped to [`MAX_SPAWN_DEPTH_CEILING`].
    #[serde(default = "default_fleet_max_spawn_depth")]
    pub max_spawn_depth: u32,
    /// Extra system prompt text appended to every headless worker.
    /// Useful for injecting org-wide policy or behavior constraints.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub append_system_prompt: String,
    /// Output format for fleet worker results.
    /// `"text"` (default) or `"stream-json"` for newline-delimited JSON events.
    #[serde(default = "default_fleet_output_format")]
    pub output_format: String,
    /// Maximum number of concurrent fleet tasks. Defaults to 8 to match
    /// `max_subagents`. Set to 1 to disable parallel execution.
    #[serde(default = "default_fleet_max_concurrent_tasks")]
    pub max_concurrent_tasks: u32,
}

fn default_fleet_max_turns() -> u32 {
    u32::MAX
}

fn default_fleet_max_spawn_depth() -> u32 {
    DEFAULT_SPAWN_DEPTH
}

fn default_fleet_output_format() -> String {
    "text".to_string()
}

fn default_fleet_max_concurrent_tasks() -> u32 {
    8
}

impl Default for FleetExecConfig {
    fn default() -> Self {
        Self {
            allowed_tools: Vec::new(),
            disallowed_tools: Vec::new(),
            max_turns: default_fleet_max_turns(),
            max_spawn_depth: default_fleet_max_spawn_depth(),
            append_system_prompt: String::new(),
            output_format: default_fleet_output_format(),
            max_concurrent_tasks: default_fleet_max_concurrent_tasks(),
        }
    }
}

/// Fleet org-chart profile.
///
/// A profile is an additive config record for future fleet scheduling policy.
/// Loading one must not grant runtime permissions by itself: shell and trust
/// escalation default off, and approvals default on.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct FleetProfile {
    /// Org-chart slot this profile describes.
    #[serde(default)]
    pub slot: FleetSlot,
    /// Semantic role name and optional instruction overlay.
    #[serde(default)]
    pub role: FleetRole,
    /// Model class / route-role hint.
    #[serde(default)]
    pub loadout: FleetLoadout,
    /// Optional explicit model id for this profile on the active/resolved route.
    ///
    /// This is not an auth or endpoint selector. Provider-scoped routing still
    /// validates the executable provider/model/wire-model decision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Permission defaults requested by the profile.
    #[serde(default)]
    pub permissions: FleetProfilePermissions,
    /// Delegation hints for future manager policy.
    #[serde(default)]
    pub delegation: FleetDelegationHints,
}

/// Semantic role declaration for a fleet profile.
///
/// TOML may use either `role = "reviewer"` or a role table with `name` and
/// `instructions`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FleetRole {
    /// Stable role name, e.g. `scout`, `implementer`, or `verifier`.
    pub name: String,
    /// Optional short description for config UIs and docs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Optional instruction overlay to apply when the role is later consumed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
}

impl Default for FleetRole {
    fn default() -> Self {
        Self {
            name: "general".to_string(),
            description: None,
            instructions: None,
        }
    }
}

impl<'de> Deserialize<'de> for FleetRole {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum FleetRoleWire {
            Name(String),
            Full {
                #[serde(default)]
                name: Option<String>,
                #[serde(default)]
                description: Option<String>,
                #[serde(default)]
                instructions: Option<String>,
            },
        }

        match FleetRoleWire::deserialize(deserializer)? {
            FleetRoleWire::Name(name) => Ok(Self {
                name,
                ..Self::default()
            }),
            FleetRoleWire::Full {
                name,
                description,
                instructions,
            } => Ok(Self {
                name: name.unwrap_or_else(|| Self::default().name),
                description,
                instructions,
            }),
        }
    }
}

/// Org-chart slot for grouping fleet profiles.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum FleetSlot {
    Manager,
    Scout,
    Implementer,
    Reviewer,
    Verifier,
    ToolHeavy,
    Operator,
    Summarizer,
    #[default]
    General,
    Custom(String),
}

impl FleetSlot {
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Manager => "manager",
            Self::Scout => "scout",
            Self::Implementer => "implementer",
            Self::Reviewer => "reviewer",
            Self::Verifier => "verifier",
            Self::ToolHeavy => "tool-heavy",
            Self::Operator => "operator",
            Self::Summarizer => "summarizer",
            Self::General => "general",
            Self::Custom(value) => value.as_str(),
        }
    }

    #[must_use]
    pub fn from_name(value: &str) -> Self {
        match value.trim() {
            "manager" | "coordinator" => Self::Manager,
            "scout" | "research" | "research-worker" => Self::Scout,
            "implementer" | "builder" => Self::Implementer,
            "reviewer" => Self::Reviewer,
            "verifier" | "tester" => Self::Verifier,
            "tool-heavy" | "tool_heavy" => Self::ToolHeavy,
            "operator" | "incident" | "incident-worker" => Self::Operator,
            "summarizer" | "reducer" => Self::Summarizer,
            "general" | "" => Self::General,
            other => Self::Custom(other.to_string()),
        }
    }
}

impl Serialize for FleetSlot {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for FleetSlot {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Ok(Self::from_name(&value))
    }
}

/// Model class or route-role hint for a profile.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum FleetLoadout {
    #[default]
    Inherit,
    Strong,
    Fast,
    Balanced,
    DeepReasoning,
    Code,
    Review,
    ToolHeavy,
    Custom(String),
}

impl FleetLoadout {
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Inherit => "inherit",
            Self::Strong => "strong",
            Self::Fast => "fast",
            Self::Balanced => "balanced",
            Self::DeepReasoning => "deep-reasoning",
            Self::Code => "code",
            Self::Review => "review",
            Self::ToolHeavy => "tool-heavy",
            Self::Custom(value) => value.as_str(),
        }
    }

    #[must_use]
    pub fn from_name(value: &str) -> Self {
        match value.trim() {
            "inherit" | "default" | "auto" | "" => Self::Inherit,
            "strong" => Self::Strong,
            "fast" => Self::Fast,
            "balanced" => Self::Balanced,
            "deep-reasoning" | "deep_reasoning" | "reasoning" => Self::DeepReasoning,
            "code" | "coding" => Self::Code,
            "review" | "reviewer" => Self::Review,
            "tool-heavy" | "tool_heavy" => Self::ToolHeavy,
            other => Self::Custom(other.to_string()),
        }
    }
}

impl Serialize for FleetLoadout {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for FleetLoadout {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Ok(Self::from_name(&value))
    }
}

/// Safe permission defaults attached to a fleet profile.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FleetProfilePermissions {
    /// Permit shell-capable tools for this profile when later consumed.
    #[serde(default)]
    pub allow_shell: bool,
    /// Permit trusted/elevated execution for this profile when later consumed.
    #[serde(default)]
    pub trust: bool,
    /// Require approval by default. This intentionally defaults on.
    #[serde(default = "default_fleet_profile_approval_required")]
    pub approval_required: bool,
}

fn default_fleet_profile_approval_required() -> bool {
    true
}

impl Default for FleetProfilePermissions {
    fn default() -> Self {
        Self {
            allow_shell: false,
            trust: false,
            approval_required: true,
        }
    }
}

/// Delegation hints for future fleet manager scheduling.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct FleetDelegationHints {
    /// Optional profile-level child spawn depth. `None` means inherit existing
    /// fleet/sub-agent config.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_spawn_depth: Option<u32>,
    /// Optional profile-level worker concurrency hint.
    #[serde(
        default,
        alias = "concurrency",
        skip_serializing_if = "Option::is_none"
    )]
    pub max_concurrency: Option<usize>,
}

/// A named role preset that bundles common worker settings.
///
/// Task specs reference a role name (e.g. `"role": "reviewer"`), and the
/// fleet manager fills in any missing fields from the preset. User-defined
/// roles in `[fleet.roles]` override built-in defaults with the same name.
///
/// Token budgets and tool-call limits are task-level decisions — they don't
/// belong on role presets. Use `timeout_seconds` as the safety bound.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FleetRolePreset {
    /// Short description of what this role is for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Default tool profile (`"read-only"`, `"read-write"`, or `"custom"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_profile: Option<String>,
    /// Default set of tool names available to this role.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<String>,
    /// Default capability tags (e.g. `"rust"`, `"git"`, `"gh"`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<String>,
    /// Default timeout in seconds for tasks using this role.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<u64>,
    /// Default trust level override for this role.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trust_level: Option<String>,
}

fn default_fleet_trust_level_str() -> String {
    "sandbox".to_string()
}

fn default_fleet_require_identity() -> bool {
    true
}

fn default_fleet_max_trust_level_str() -> String {
    "operator".to_string()
}

impl Default for FleetConfigToml {
    fn default() -> Self {
        Self {
            default_trust_level: default_fleet_trust_level_str(),
            require_identity_verification: default_fleet_require_identity(),
            max_trust_level: default_fleet_max_trust_level_str(),
            roles: BTreeMap::new(),
            profiles: BTreeMap::new(),
            exec: FleetExecConfig::default(),
        }
    }
}

impl FleetConfigToml {
    /// Resolve a role preset by name. Checks user-defined roles first,
    /// then falls back to built-in role defaults.
    #[must_use]
    pub fn resolve_role(&self, name: &str) -> Option<FleetRolePreset> {
        self.roles
            .get(name)
            .cloned()
            .or_else(|| built_in_role_presets().get(name).cloned())
    }
}

/// Built-in role presets that are always available without config.
#[must_use]
pub fn built_in_role_presets() -> BTreeMap<String, FleetRolePreset> {
    [
        (
            "smoke-runner".to_string(),
            FleetRolePreset {
                description: Some("Lightweight read-only smoke check worker".to_string()),
                tool_profile: Some("read-only".to_string()),
                tools: vec![],
                capabilities: vec![],
                timeout_seconds: Some(300),
                trust_level: Some("local".to_string()),
            },
        ),
        (
            "reviewer".to_string(),
            FleetRolePreset {
                description: Some("Read-only code and documentation review".to_string()),
                tool_profile: Some("read-only".to_string()),
                tools: vec![],
                capabilities: vec![],
                timeout_seconds: Some(600),
                trust_level: None,
            },
        ),
        (
            "builder".to_string(),
            FleetRolePreset {
                description: Some(
                    "Read-write builder with compilation and test access".to_string(),
                ),
                tool_profile: Some("read-write".to_string()),
                tools: vec![],
                capabilities: vec![],
                timeout_seconds: Some(1800),
                trust_level: Some("local".to_string()),
            },
        ),
        (
            "read-only".to_string(),
            FleetRolePreset {
                description: Some(
                    "Minimal read-only observer with no writes or secrets".to_string(),
                ),
                tool_profile: Some("read-only".to_string()),
                tools: vec![],
                capabilities: vec![],
                timeout_seconds: Some(300),
                trust_level: Some("sandbox".to_string()),
            },
        ),
    ]
    .into()
}
