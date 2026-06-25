use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::approval::{NetworkApprovalContext, NetworkPolicyAmendment, ReviewDecision};
use crate::thread::ThreadGoal;

/// Status of an MCP server during startup.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpStartupStatus {
    /// The server is in the process of starting.
    Starting,
    /// The server is ready to accept requests.
    Ready,
    /// The server failed to start.
    Failed { error: String },
    /// Startup was cancelled.
    Cancelled,
}

/// A progress update for a single MCP server's startup.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpStartupUpdateEvent {
    /// Name of the MCP server.
    pub server_name: String,
    /// Current startup status.
    pub status: McpStartupStatus,
}

/// Details of an MCP server that failed to start.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpStartupFailure {
    /// Name of the MCP server that failed.
    pub server_name: String,
    /// Error description.
    pub error: String,
}

/// Summary event emitted once all MCP servers have finished starting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpStartupCompleteEvent {
    /// Servers that started successfully.
    pub ready: Vec<String>,
    /// Servers that failed to start.
    pub failed: Vec<McpStartupFailure>,
    /// Servers whose startup was cancelled.
    pub cancelled: Vec<String>,
}

/// A selectable option presented to the user in a clarification question.
///
/// Headless serialization shape for the `request_user_input` model tool,
/// mirrored after the TUI's `UserInputOption`. Shared by the
/// [`EventFrame::UserInputRequest`] frame and the [`AppRequest::SubmitUserInput`]
/// reply path so both surfaces agree on the question schema.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserInputOptionEvent {
    /// Short label for the option (also the value submitted when picked).
    pub label: String,
    /// Longer description shown alongside the label.
    pub description: String,
}

/// A single clarification question posed to the user.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserInputQuestionEvent {
    /// Compact header shown as the question title.
    pub header: String,
    /// Stable identifier used to correlate answers back to this question.
    pub id: String,
    /// The question body.
    pub question: String,
    /// 2-4 suggested answers.
    pub options: Vec<UserInputOptionEvent>,
    /// When `true`, the client should also offer a free-text response.
    #[serde(default)]
    pub allow_free_text: bool,
    /// When `true`, the user may select more than one option.
    #[serde(default)]
    pub multi_select: bool,
}

/// An event requesting structured user input via a model-tool call.
///
/// Sibling of [`ExecApprovalRequestEvent`] for the clarification-question
/// flow. Emitted fire-and-return by `Runtime::invoke_tool` when the model
/// invokes `request_user_input` in a headless context.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserInputRequestEvent {
    /// Identifier of the tool call requesting input.
    pub call_id: String,
    /// The turn during which the request was made.
    pub turn_id: String,
    /// Unique identifier for this user-input request (clients reply with it).
    pub request_id: String,
    /// 1-3 questions to present.
    pub questions: Vec<UserInputQuestionEvent>,
}

/// One answer to a clarification question.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserInputAnswerEvent {
    /// The `id` of the question this answer corresponds to.
    pub id: String,
    /// The selected option's label, or `"Other"` for a free-text response.
    pub label: String,
    /// The resolved value (option label, or the typed free-text).
    pub value: String,
}

/// An event requesting user approval for a command execution or patch application.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecApprovalRequestEvent {
    /// Identifier of the tool call requesting approval.
    pub call_id: String,
    /// Unique identifier for this approval request.
    pub approval_id: String,
    /// The turn during which the request was made.
    pub turn_id: String,
    /// The command that would be executed.
    pub command: String,
    /// The working directory for the command.
    pub cwd: String,
    /// Human-readable reason why approval is needed.
    pub reason: String,
    /// Policy rule that matched this approval request, when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matched_rule: Option<Box<str>>,
    /// Network context if the approval involves network access.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network_approval_context: Option<NetworkApprovalContext>,
    /// Proposed execution policy rule amendments.
    #[serde(default)]
    pub proposed_execpolicy_amendment: Vec<String>,
    /// Proposed network policy amendments.
    #[serde(default)]
    pub proposed_network_policy_amendments: Vec<NetworkPolicyAmendment>,
    /// Additional permissions being requested.
    #[serde(default)]
    pub additional_permissions: Vec<String>,
    /// The set of decisions the user can choose from.
    #[serde(default)]
    pub available_decisions: Vec<ReviewDecision>,
}

/// The channel a response delta is being written to.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResponseChannel {
    /// The main visible text output.
    #[default]
    Text,
    /// Internal reasoning / chain-of-thought output.
    Reasoning,
}

impl ResponseChannel {
    /// Returns `true` if this is the `Text` channel.
    pub const fn is_text(&self) -> bool {
        matches!(self, ResponseChannel::Text)
    }
}

/// A single streaming event frame emitted during agent execution.
///
/// Events are tagged by the `event` field and cover the full lifecycle of a
/// turn: response streaming, tool calls, MCP lifecycle, command execution,
/// patch application, approvals, and errors.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum EventFrame {
    /// A new model response has started.
    ResponseStart { response_id: String },
    /// A incremental text delta for an in-progress response.
    ResponseDelta {
        response_id: String,
        delta: String,
        #[serde(default, skip_serializing_if = "ResponseChannel::is_text")]
        channel: ResponseChannel,
    },
    /// The model response has finished.
    ResponseEnd { response_id: String },
    /// A tool call has begun.
    ToolCallStart {
        response_id: String,
        tool_name: String,
        arguments: Value,
    },
    /// A tool call has completed and produced a result.
    ToolCallResult {
        response_id: String,
        tool_name: String,
        output: Value,
    },
    /// Progress update for an MCP server starting up.
    McpStartupUpdate { update: McpStartupUpdateEvent },
    /// All MCP servers have finished starting.
    McpStartupComplete { summary: McpStartupCompleteEvent },
    /// An MCP tool call has begun.
    McpToolCallBegin {
        server_name: String,
        tool_name: String,
    },
    /// An MCP tool call has finished.
    McpToolCallEnd {
        server_name: String,
        tool_name: String,
        ok: bool,
    },
    /// User approval is needed for a command execution.
    ExecApprovalRequest { request: ExecApprovalRequestEvent },
    /// User approval is needed for applying a patch.
    ApplyPatchApprovalRequest { request: ExecApprovalRequestEvent },
    /// A model tool is requesting structured clarification input from the user.
    ///
    /// Headless sibling of the TUI's `request_user_input` modal flow.
    /// `request_id` correlates with an [`AppRequest::SubmitUserInput`] reply.
    UserInputRequest { request: UserInputRequestEvent },
    /// An MCP server is requesting user input (elicitation).
    ElicitationRequest {
        server_name: String,
        request_id: String,
        prompt: String,
    },
    /// A command has started executing.
    ExecCommandBegin { command: String, cwd: String },
    /// Incremental output from a running command.
    ExecCommandOutputDelta { command: String, delta: String },
    /// A command has finished executing.
    ExecCommandEnd { command: String, exit_code: i32 },
    /// A patch has started being applied to a file.
    PatchApplyBegin { path: String },
    /// A patch has finished being applied.
    PatchApplyEnd { path: String, ok: bool },
    /// A new turn has started within a thread.
    TurnStarted { turn_id: String },
    /// A turn has completed successfully.
    TurnComplete { turn_id: String },
    /// A turn was aborted before completion.
    TurnAborted { turn_id: String, reason: String },
    /// A thread goal was set or updated.
    ThreadGoalUpdated { goal: ThreadGoal },
    /// A thread goal was cleared.
    ThreadGoalCleared { thread_id: String },
    /// An error occurred during processing.
    Error {
        response_id: String,
        message: String,
    },
}
