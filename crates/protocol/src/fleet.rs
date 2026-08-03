//! Agent Fleet control-plane protocol types.
//!
//! These types define the durable, serializable contract between the fleet
//! manager, workers, CLI/TUI surfaces, and the Runtime API. They are
//! intentionally additive: existing runtime-event consumers ignore unknown
//! fields and are unaffected by fleet extensions.
//!
//! See:
//! - <https://github.com/XiaomingX/mimofan/issues/3154> (Agent Fleet control plane)
//! - <https://github.com/XiaomingX/mimofan/issues/3096> (Runtime API sub-agent direction)

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use serde_json::Value;

pub const FLEET_PROTOCOL_VERSION: &str = "0.1.0";

/// Globally unique identifier for a fleet run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct FleetRunId(pub String);

impl From<String> for FleetRunId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for FleetRunId {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

/// Top-level fleet run handle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FleetRun {
    pub id: FleetRunId,
    pub name: String,
    pub status: FleetRunStatus,
    #[serde(default)]
    pub task_specs: Vec<FleetTaskSpec>,
    #[serde(default)]
    pub worker_specs: Vec<FleetWorkerSpec>,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub security_policy: Option<FleetSecurityPolicy>,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
}

/// Lifecycle status for an entire fleet run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FleetRunStatus {
    Pending,
    Queued,
    Running,
    Paused,
    Completed,
    Failed,
    Cancelled,
}

/// Specification of a single unit of work within a run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FleetTaskSpec {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub objective: Option<String>,
    pub instructions: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worker: Option<FleetTaskWorkerProfile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace: Option<FleetWorkspaceRequirements>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub input_files: Vec<PathBuf>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub context: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget: Option<FleetTaskBudget>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default)]
    pub expected_artifacts: Vec<FleetArtifactKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scorer: Option<FleetScorerSpec>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_policy: Option<FleetRetryPolicy>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alert_policy: Option<FleetAlertPolicy>,
    #[serde(default)]
    pub timeout_seconds: Option<u64>,
    #[serde(default)]
    pub metadata: BTreeMap<String, Value>,
}

/// Worker role and tool expectations for a task.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct FleetTaskWorkerProfile {
    /// Named agent profile/persona posture to layer onto this worker.
    ///
    /// `profile` is accepted as a shorter authoring alias. This is an intent
    /// reference only; profile loading and permission narrowing happen in the
    /// Fleet runtime layer.
    #[serde(default, alias = "profile", skip_serializing_if = "Option::is_none")]
    pub agent_profile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// Fleet loadout intent such as `auto`, `fast`, or `review`.
    ///
    /// This is not a concrete provider/model selection; route resolution owns
    /// the executable provider/model/wire-model decision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loadout: Option<String>,
    /// Fleet model class hint such as `strong`, `balanced`, or `fast`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_class: Option<String>,
    /// Optional explicit model id for this worker.
    ///
    /// Task-level model overrides are visible authoring data and take
    /// precedence over the referenced agent profile's model hint. Provider and
    /// wire-model validation still belong to route resolution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_profile: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<String>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<String>,
}

/// Workspace and environment constraints needed before a task starts.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct FleetWorkspaceRequirements {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root: Option<PathBuf>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub required_files: Vec<PathBuf>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub writable_paths: Vec<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment: Option<FleetEnvironmentRequirements>,
}

/// Environment variables a task requires or may pass through to workers.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct FleetEnvironmentRequirements {
    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub required: Vec<String>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub allowlist: Vec<String>,
}

/// Budget limits for a task.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct FleetTaskBudget {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tool_calls: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_seconds: Option<u64>,
}

/// Reference to an artifact produced or consumed by a task.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FleetArtifactRef {
    pub kind: FleetArtifactKind,
    pub path: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checksum: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(default)]
    pub size_bytes: Option<u64>,
}

/// Kind of artifact a task may produce or consume.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FleetArtifactKind {
    Log,
    Patch,
    TestResult,
    Report,
    Checkpoint,
    Receipt,
    Other(String),
}

impl FleetArtifactKind {
    fn as_wire_str(&self) -> &str {
        match self {
            Self::Log => "log",
            Self::Patch => "patch",
            Self::TestResult => "test_result",
            Self::Report => "report",
            Self::Checkpoint => "checkpoint",
            Self::Receipt => "receipt",
            Self::Other(kind) => kind.as_str(),
        }
    }

    fn from_wire_str(value: &str) -> Self {
        match value {
            "log" => Self::Log,
            "patch" => Self::Patch,
            "test_result" => Self::TestResult,
            "report" => Self::Report,
            "checkpoint" => Self::Checkpoint,
            "receipt" => Self::Receipt,
            other => Self::Other(other.to_string()),
        }
    }
}

impl Serialize for FleetArtifactKind {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_wire_str())
    }
}

impl<'de> Deserialize<'de> for FleetArtifactKind {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Ok(Self::from_wire_str(&value))
    }
}

/// Scoring rule used to verify a task result.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FleetScorerSpec {
    ExitCode,
    FileExists {
        path: PathBuf,
    },
    RegexMatch {
        path: PathBuf,
        pattern: String,
    },
    JsonPath {
        path: PathBuf,
        expression: String,
    },
    Command {
        command: String,
        #[serde(default)]
        args: Vec<String>,
    },
    MimofanVerifierPrompt {
        prompt: String,
    },
    Manual,
}

/// Worker specification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FleetWorkerSpec {
    pub id: String,
    pub name: String,
    pub host: FleetHostSpec,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trust_level: Option<FleetTrustLevel>,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_concurrent_tasks: Option<usize>,
}

/// Host on which a worker runs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FleetHostSpec {
    Local,
    Ssh {
        host: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        port: Option<u16>,
        #[serde(skip_serializing_if = "Option::is_none")]
        user: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        identity: Option<PathBuf>,
        /// Known hosts file for host-key verification.
        #[serde(skip_serializing_if = "Option::is_none")]
        known_hosts: Option<PathBuf>,
        /// Expected host key fingerprint (SHA256:...) for key pinning.
        /// When set, the connection is only trusted if the server's
        /// host key matches this fingerprint exactly.
        #[serde(skip_serializing_if = "Option::is_none")]
        host_key_fingerprint: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        working_directory: Option<PathBuf>,
        #[serde(default)]
        #[serde(skip_serializing_if = "Vec::is_empty")]
        env_allowlist: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        mimofan_binary: Option<String>,
    },
    #[serde(alias = "container")]
    #[serde(alias = "Container")]
    Docker {
        image: String,
        #[serde(default)]
        args: Vec<String>,
    },
}

// ── Security and trust types ────────────────────────────────────────────────

/// Trust classification assigned to a worker host.
///
/// The trust level determines what a worker is allowed to do and what
/// secrets it may access. The default for new workers is [`FleetTrustLevel::Sandbox`];
/// operators must explicitly raise trust for SSH or container workers.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Default)]
#[serde(rename_all = "snake_case")]
pub enum FleetTrustLevel {
    /// Fully isolated: no network, no secrets, no writes outside `.mimofan/fleet/`.
    /// Suitable for untrusted code review, community PR checks, or third-party tool runs.
    #[default]
    Sandbox = 0,
    /// Local-only worker with access to the workspace and configured secrets.
    /// Default for local workers. May read repo files but writes are gated.
    Local = 1,
    /// Worker on a known remote host with verified identity and a bounded
    /// set of explicitly granted capabilities. Requires SSH host-key
    /// verification or equivalent attestation.
    #[serde(alias = "remote-verified", alias = "remoteVerified")]
    RemoteVerified = 2,
    /// Fully trusted worker (e.g. operator's own machine, CI runner).
    /// Has access to all configured secrets and may perform any action the
    /// operator can. Reserved for dogfood smoke and operator-owned machines.
    Operator = 3,
}

impl FleetTrustLevel {
    /// Whether this trust level is allowed to access provider secrets.
    #[must_use]
    pub fn may_access_secrets(&self) -> bool {
        matches!(self, Self::Operator | Self::RemoteVerified | Self::Local)
    }

    /// Whether this trust level is allowed to write outside `.mimofan/fleet/`.
    #[must_use]
    pub fn may_write_workspace(&self) -> bool {
        matches!(self, Self::Operator | Self::Local)
    }

    /// Whether this trust level is allowed network access.
    #[must_use]
    pub fn may_access_network(&self) -> bool {
        matches!(self, Self::Operator | Self::RemoteVerified | Self::Local)
    }
}

/// Security policy applied to a fleet run.
///
/// A policy defines the default trust level for workers, which secrets
/// may be resolved, and what capabilities are granted. When a run has no
/// explicit policy, workers inherit conservative defaults.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FleetSecurityPolicy {
    /// Default trust level for workers that don't declare one explicitly.
    #[serde(default)]
    pub default_trust_level: FleetTrustLevel,
    /// Secret refs that workers may resolve. An empty list means no secrets
    /// are available. Each entry is a key name, not a value.
    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub allowed_secrets: Vec<FleetSecretRef>,
    /// Capability grants for workers in this run.
    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub capability_grants: Vec<FleetCapabilityGrant>,
    /// Maximum trust level any worker in this run may have, even if the
    /// worker spec requests higher. Defaults to Operator (no ceiling).
    #[serde(default = "default_max_trust_level")]
    pub max_trust_level: FleetTrustLevel,
    /// Require identity verification for remote workers. When true, SSH
    /// workers must pass host-key verification before being trusted at
    /// RemoteVerified level; unverified remotes stay at Sandbox.
    #[serde(default)]
    pub require_identity_verification: bool,
    /// Allow conservative parallel execution of read-only tools (#2983).
    /// When true, workers may batch independent read-only tool calls
    /// (reads, searches, greps) into concurrent turns. Disabled by default
    /// to avoid overwhelming providers or hitting rate limits.
    #[serde(default)]
    pub allow_parallel_reads: bool,
}

fn default_max_trust_level() -> FleetTrustLevel {
    FleetTrustLevel::Operator
}

impl Default for FleetSecurityPolicy {
    fn default() -> Self {
        Self {
            default_trust_level: FleetTrustLevel::Sandbox,
            allowed_secrets: Vec::new(),
            capability_grants: Vec::new(),
            max_trust_level: FleetTrustLevel::Operator,
            require_identity_verification: false,
            allow_parallel_reads: false,
        }
    }
}

/// A reference to a secret that should be resolved at runtime, never
/// serialized as a plaintext value.
///
/// Secret refs appear in task specs, alert configs, and worker definitions.
/// The actual secret value is resolved by the fleet manager from the
/// secrets backend (OS keyring, environment, or file store) just before
/// the worker starts.
#[derive(Debug, Clone, Serialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FleetSecretRef {
    /// The secret key name (e.g. `"MIMOFAN_API_KEY"`, `"GH_TOKEN"`).
    pub key: String,
    /// Optional source hint for resolution order.
    /// - `"env"` — resolve from environment variable
    /// - `"keyring"` — resolve from OS keyring
    /// - `"file"` — resolve from `~/.mimofan/secrets/`
    /// - absent / null — try all sources in default order
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

impl FleetSecretRef {
    /// Create a secret ref from a key name with default resolution.
    #[must_use]
    pub fn new(key: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            source: None,
        }
    }

    /// Create a secret ref with an explicit source.
    #[must_use]
    pub fn with_source(key: impl Into<String>, source: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            source: Some(source.into()),
        }
    }

    /// Redacted display form for logging. Shows the key name and source
    /// but never the resolved value.
    #[must_use]
    pub fn redacted(&self) -> String {
        match &self.source {
            Some(src) => format!("<secret:{}.{}>", src, self.key),
            None => format!("<secret:{}>", self.key),
        }
    }
}

impl std::fmt::Display for FleetSecretRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.redacted())
    }
}

impl From<&str> for FleetSecretRef {
    fn from(key: &str) -> Self {
        Self::new(key)
    }
}

impl From<String> for FleetSecretRef {
    fn from(key: String) -> Self {
        Self::new(key)
    }
}

impl<'de> Deserialize<'de> for FleetSecretRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum SecretRefWire {
            Key(String),
            Structured {
                key: String,
                #[serde(default)]
                source: Option<String>,
            },
        }

        match SecretRefWire::deserialize(deserializer)? {
            SecretRefWire::Key(key) if !key.trim().is_empty() => Ok(FleetSecretRef::new(key)),
            SecretRefWire::Key(_) => Err(de::Error::custom("secret ref key cannot be empty")),
            SecretRefWire::Structured { key, source } if !key.trim().is_empty() => {
                Ok(FleetSecretRef { key, source })
            }
            SecretRefWire::Structured { .. } => {
                Err(de::Error::custom("secret ref key cannot be empty"))
            }
        }
    }
}

/// How a worker authenticates to the fleet manager.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum FleetWorkerAuth {
    /// No authentication (local workers share the same uid).
    None,
    /// SSH key-based authentication with host-key verification.
    SshKey {
        /// Path to the SSH identity file (may be a FleetSecretRef in JSON
        /// as `{"key": "...", "source": "file"}`).
        identity: PathBuf,
        /// Known hosts file for host-key verification.
        #[serde(skip_serializing_if = "Option::is_none")]
        known_hosts: Option<PathBuf>,
        /// Expected host key fingerprint for pinning.
        #[serde(skip_serializing_if = "Option::is_none")]
        host_key_fingerprint: Option<String>,
        /// SSH user for the connection.
        #[serde(skip_serializing_if = "Option::is_none")]
        user: Option<String>,
    },
    /// Token-based authentication for remote workers behind a fleet proxy.
    Token {
        /// Reference to the token secret.
        token_ref: FleetSecretRef,
    },
    /// mTLS certificate-based authentication.
    Mtls {
        /// Path to the client certificate.
        cert_path: PathBuf,
        /// Reference to the private key secret.
        key_ref: FleetSecretRef,
    },
}

/// A capability grant that explicitly authorizes a worker to perform
/// a specific class of action.
///
/// By default, new workers get no grants (least privilege). Grants are
/// additive: a worker's effective capabilities are the union of its
/// trust-level defaults plus any explicit grants.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FleetCapabilityGrant {
    /// The capability being granted (e.g. `"network"`, `"git-push"`,
    /// `"provider-secrets"`, `"release"`).
    pub capability: String,
    /// Optional scope limiting the grant (e.g. `"github.com"` for network,
    /// `"crates/tui/**"` for file writes).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    /// Optional justification for the grant (audit trail).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Runtime status of a worker.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FleetWorkerStatus {
    Unknown,
    Online,
    Busy,
    Offline,
    Unhealthy,
    Draining,
    Retired,
}

/// Durable inbox entry: a task waiting to be leased to a worker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FleetInboxEntry {
    pub run_id: FleetRunId,
    pub task_id: String,
    pub priority: i32,
    pub enqueued_at: String,
    #[serde(default)]
    pub lease_deadline: Option<String>,
    #[serde(default)]
    pub attempts: u32,
}

/// Worker event envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FleetWorkerEvent {
    pub seq: u64,
    pub run_id: FleetRunId,
    pub worker_id: String,
    pub task_id: String,
    pub timestamp: String,
    #[serde(flatten)]
    pub payload: FleetWorkerEventPayload,
    #[serde(default)]
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: BTreeMap<String, Value>,
}

/// Union of all worker event payloads.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum FleetWorkerEventPayload {
    Queued,
    Leased {
        #[serde(skip_serializing_if = "Option::is_none")]
        lease_expires_at: Option<String>,
    },
    Starting,
    Running,
    ModelWait {
        #[serde(skip_serializing_if = "Option::is_none")]
        model: Option<String>,
    },
    RunningTool {
        tool: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        call_id: Option<String>,
    },
    Heartbeat {
        #[serde(default)]
        #[serde(skip_serializing_if = "Option::is_none")]
        cpu_percent: Option<f32>,
        #[serde(default)]
        #[serde(skip_serializing_if = "Option::is_none")]
        memory_mb: Option<u64>,
    },
    Artifact(FleetArtifactRef),
    Completed {
        #[serde(default)]
        #[serde(skip_serializing_if = "Option::is_none")]
        exit_code: Option<i32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        summary: Option<String>,
    },
    Failed {
        reason: String,
        #[serde(default)]
        recoverable: bool,
    },
    Cancelled {
        #[serde(skip_serializing_if = "Option::is_none")]
        cancelled_by: Option<String>,
    },
    Interrupted {
        #[serde(skip_serializing_if = "Option::is_none")]
        signal: Option<String>,
    },
    Stale {
        #[serde(skip_serializing_if = "Option::is_none")]
        last_heartbeat_at: Option<String>,
    },
    Restarted {
        #[serde(default)]
        restart_count: u32,
    },
    Escalated {
        channel: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        alert_id: Option<String>,
    },
}

/// Retry policy for a task or worker.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FleetRetryPolicy {
    #[serde(default = "default_retry_max_attempts")]
    pub max_attempts: u32,
    #[serde(default = "default_retry_initial_backoff_seconds")]
    pub initial_backoff_seconds: u64,
    #[serde(default = "default_retry_max_backoff_seconds")]
    pub max_backoff_seconds: u64,
    #[serde(default = "default_retry_backoff_multiplier")]
    pub backoff_multiplier: u32,
}

impl Default for FleetRetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_backoff_seconds: 5,
            max_backoff_seconds: 300,
            backoff_multiplier: 2,
        }
    }
}

fn default_retry_max_attempts() -> u32 {
    FleetRetryPolicy::default().max_attempts
}

fn default_retry_initial_backoff_seconds() -> u64 {
    FleetRetryPolicy::default().initial_backoff_seconds
}

fn default_retry_max_backoff_seconds() -> u64 {
    FleetRetryPolicy::default().max_backoff_seconds
}

fn default_retry_backoff_multiplier() -> u32 {
    FleetRetryPolicy::default().backoff_multiplier
}

/// Alert/escalation policy attached to a task or run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FleetAlertPolicy {
    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<FleetAlertEventClass>,
    #[serde(default)]
    pub channels: Vec<FleetAlertChannel>,
    #[serde(default)]
    pub after_attempts: Option<u32>,
    #[serde(default)]
    pub after_minutes_stale: Option<u64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum FleetAlertEventClass {
    Stale,
    RestartExhausted,
    NeedsHuman,
    BudgetExceeded,
    VerifierFailed,
    RunCompleted,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FleetAlertChannel {
    Slack {
        /// Webhook URL, resolved from a secret ref or inline.
        #[serde(flatten)]
        webhook: FleetAlertEndpoint,
    },
    Webhook {
        #[serde(flatten)]
        endpoint: FleetAlertEndpoint,
    },
    #[serde(alias = "pager_duty")]
    #[serde(alias = "pagerduty")]
    PagerDuty {
        routing_key: String,
        severity: String,
    },
}

/// An alert channel endpoint, supporting both inline URLs and secret refs.
///
/// For Slack and generic webhook channels, the URL may be provided directly
/// or as a secret reference resolved at send time. When both `url` and
/// `url_ref` are present, `url_ref` takes precedence after resolution.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FleetAlertEndpoint {
    /// Inline URL (plaintext; only for non-sensitive endpoints).
    #[serde(
        alias = "webhook_url",
        alias = "endpoint_url",
        skip_serializing_if = "Option::is_none"
    )]
    pub url: Option<String>,
    /// Reference to a secret containing the webhook URL.
    #[serde(
        alias = "webhook_url_ref",
        alias = "webhook_ref",
        alias = "url_secret_ref",
        skip_serializing_if = "Option::is_none"
    )]
    pub url_ref: Option<FleetSecretRef>,
    /// Optional HMAC secret for webhook payload signing, as a secret ref.
    #[serde(
        alias = "secret",
        alias = "webhook_secret",
        alias = "signing_secret",
        skip_serializing_if = "Option::is_none"
    )]
    pub secret_ref: Option<FleetSecretRef>,
}

impl FleetAlertEndpoint {
    /// Create an inline URL endpoint (for non-sensitive use).
    #[must_use]
    pub fn inline(url: impl Into<String>) -> Self {
        Self {
            url: Some(url.into()),
            url_ref: None,
            secret_ref: None,
        }
    }

    /// Create a secret-backed URL endpoint.
    #[must_use]
    pub fn from_secret(url_ref: FleetSecretRef) -> Self {
        Self {
            url: None,
            url_ref: Some(url_ref),
            secret_ref: None,
        }
    }

    /// Redacted display form for logging.
    #[must_use]
    pub fn redacted(&self) -> String {
        self.url_ref
            .as_ref()
            .map_or_else(|| "<inline-url>".to_string(), |r| r.redacted())
    }
}

/// Resolved-route detail persisted on a [`FleetReceipt`] (#3154).
///
/// This is an additive, *plain-strings* snapshot of the route a fleet worker
/// resolved to. It deliberately does NOT depend on any `mimofan-config` route
/// type so the protocol crate stays free of the route model.
///
/// CRITICAL no-secrets invariant: this struct carries ONLY non-sensitive route
/// shape — provider id/kind, model ids, wire protocol, role/loadout intent, and
/// the resolution source. It must NEVER hold a credential, API key, bearer
/// token, or a base URL that embeds credentials. There is intentionally no
/// field that could carry a secret.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FleetResolvedRoute {
    /// Resolved provider canonical id (e.g. `"deepseek"`).
    pub provider_id: String,
    /// Resolved provider kind (e.g. `"deepseek"`).
    pub provider_kind: String,
    /// Canonical, provider-agnostic model identity, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_model: Option<String>,
    /// Provider-owned wire model id placed on the request.
    pub wire_model_id: String,
    /// Selected wire protocol (e.g. `"chat_completions"`).
    pub protocol: String,
    /// Effective Fleet role intent, when one applied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// Effective Fleet loadout intent, when one applied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loadout: Option<String>,
    /// How the route was produced (e.g. `"resolver"`).
    pub source: String,
}

/// Receipt produced when a task completes verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FleetReceipt {
    pub run_id: FleetRunId,
    pub task_id: String,
    pub worker_id: String,
    pub completed_at: String,
    pub result: FleetTaskResult,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_kind: Option<FleetTaskFailureKind>,
    #[serde(default)]
    pub artifacts: Vec<FleetArtifactRef>,
    #[serde(default)]
    pub score: Option<FleetScore>,
    /// Resolved-route snapshot for this task (#3154).
    ///
    /// `#[serde(default)]` keeps older ledgers (written before this field
    /// existed) deserializable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_route: Option<FleetResolvedRoute>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FleetTaskResult {
    Pass,
    Partial,
    Fail,
    Skip,
    Timeout,
}

/// Source category for a failed task receipt.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FleetTaskFailureKind {
    Transport,
    Task,
    Verifier,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FleetScore {
    pub value: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}
