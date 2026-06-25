use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Classification of tool invocation origin.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolKind {
    /// A built-in function tool.
    Function,
    /// An MCP (Model Context Protocol) tool.
    Mcp,
}

/// Parameters for executing a local shell command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalShellParams {
    /// The shell command to execute.
    pub command: String,
    /// Working directory for the command.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Timeout in milliseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}

/// The payload of a tool call, discriminated by tool type.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolPayload {
    /// A built-in function call with JSON-encoded arguments.
    Function { arguments: String },
    /// A custom tool invocation with a free-form input string.
    Custom { input: String },
    /// A local shell command execution.
    LocalShell { params: LocalShellParams },
    /// An MCP tool invocation targeting a specific server and tool.
    Mcp {
        server: String,
        tool: String,
        raw_arguments: Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        raw_tool_call_id: Option<String>,
    },
}

/// The result of a tool call, discriminated by tool type.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolOutput {
    /// Result of a built-in function call.
    Function {
        /// The output body, if any.
        #[serde(skip_serializing_if = "Option::is_none")]
        body: Option<Value>,
        /// Whether the call succeeded.
        success: bool,
    },
    /// Result of an MCP tool call.
    Mcp {
        /// The result value returned by the MCP server.
        result: Value,
    },
}
