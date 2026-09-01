//! MCP server implementation for exposing API tools over stdio.

use std::collections::{HashMap, HashSet};
use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::runtime::Runtime;
use uuid::Uuid;

use crate::mcp_server_backend::{McpBackend, RealMcpBackend};
use crate::models::{ContentBlock, Message, MessageRequest, MessageResponse};
use crate::tools::spec::{ToolError, ToolResult};
use crate::tools::{ToolContext, ToolRegistryBuilder};

#[derive(Debug, Default, Deserialize)]
struct McpServerConfigFile {
    #[serde(default)]
    server: McpServerSection,
}

#[derive(Debug, Default, Deserialize)]
struct McpServerSection {
    expose_tools: Option<Vec<String>>,
    require_approval: Option<bool>,
}

#[derive(Debug, Clone)]
struct McpServerSettings {
    expose_tools: Vec<String>,
    require_approval: bool,
}

impl McpServerSettings {
    fn load() -> Result<Self> {
        let path = default_config_path();
        if let Some(path) = path.filter(|p| p.exists()) {
            let contents = std::fs::read_to_string(&path)
                .with_context(|| format!("Failed to read MCP server config: {}", path.display()))?;
            let config: McpServerConfigFile = toml::from_str(&contents).with_context(|| {
                format!("Failed to parse MCP server config: {}", path.display())
            })?;
            let expose_tools = config
                .server
                .expose_tools
                .unwrap_or_else(default_expose_tools);
            let require_approval = config.server.require_approval.unwrap_or(false);
            Ok(Self {
                expose_tools,
                require_approval,
            })
        } else {
            Ok(Self {
                expose_tools: default_expose_tools(),
                require_approval: false,
            })
        }
    }
}

#[derive(Debug, Clone)]
struct ExposedTool {
    public: String,
    internal: String,
}

pub fn run_mcp_server(workspace: PathBuf) -> Result<()> {
    let settings = McpServerSettings::load()?;
    // 组合根：注入默认后端实现，mcp_server 只依赖端口抽象（不在本模块直接耦合 client/config/session_manager）。
    let backend: Box<dyn McpBackend> = Box::new(RealMcpBackend);
    let mut server = McpServer::new(workspace, settings, backend)?;
    server.run()
}

struct McpServer {
    workspace: PathBuf,
    registry: crate::tools::ToolRegistry,
    exposed_tools: Vec<ExposedTool>,
    require_approval: bool,
    /// Thread-based conversation state for deepseek/mimofan-reply tools.
    /// Maps thread_id -> ordered list of messages in the conversation.
    ///
    /// 使用 `std::sync::Mutex` 是有意为之：`threads` 仅在同步 `handle_api_call`
    /// 内被读写（见 :380/:431），锁在同步段内持有、不跨 `.await`，且已做中毒恢复
    /// `unwrap_or_else(|e| e.into_inner())`。⚠️ 若将来 `handle_api_call` 改为 async
    /// 并在持锁期间 await，应整体换为 `tokio::sync::Mutex`。详见 ARCHITECTURE_STABILITY.md §8.3。
    threads: Arc<Mutex<HashMap<String, Vec<Message>>>>,
    /// Monotonic request counter for notification correlation.
    next_notification_id: u64,
    /// 依赖反转：运行期能力（配置加载 / API 客户端 / 会话列举）经端口注入，
    /// mcp_server 不再直接耦合 `client` / `config` / `session_manager` 等平级实现模块。
    backend: Box<dyn McpBackend>,
    /// T5: trajectory sink recording external tools/call events into
    /// `~/.mimofan/tasks/mcp-server/session.jsonl`.
    trajectory: Option<crate::core::engine::trace::SessionEventSink>,
}

/// Open (best-effort) the external-trajectory sink:
/// `~/.mimofan/tasks/mcp-server/session.jsonl`, raw (unredacted) so driving
/// harnesses can score local-tool results. None when home is unresolvable or
/// creation fails.
fn open_external_trajectory() -> Option<crate::core::engine::trace::SessionEventSink> {
    let home = dirs::home_dir()?;
    let path = home
        .join(".mimofan")
        .join("tasks")
        .join("mcp-server")
        .join("session.jsonl");
    crate::core::engine::trace::SessionEventSink::open_at(&path).ok()
}

impl McpServer {
    fn new(
        workspace: PathBuf,
        settings: McpServerSettings,
        backend: Box<dyn McpBackend>,
    ) -> Result<Self> {
        let exposed_tools = build_exposed_tools(&settings.expose_tools);
        let mut internal_names: HashSet<String> = HashSet::new();
        for tool in &exposed_tools {
            internal_names.insert(tool.internal.clone());
        }

        // Issue #746: expose the mimofan agent tool surface to external MCP
        // clients. Defaults to read-only (no shell) so clients get file/search/
        // git/LSP/diagnostics without arbitrary command execution; shell tools
        // are added only when the configured exposed-tools list opts in.
        let shell_policy = if internal_names.contains("exec_shell") {
            crate::worker_profile::ShellPolicy::Full
        } else {
            crate::worker_profile::ShellPolicy::ReadOnly
        };
        let mut builder = ToolRegistryBuilder::new()
            .with_agent_tools_policy(shell_policy)
            // Loop v4 / T5: expose the full SAST/DAST security surface over
            // MCP so an external agent tool backend (e.g. Claude Code via MCP)
            // can drive vulnerability analysis with NO mimofan SK: mimofan
            // only runs local tools, no LLM calls.
            .with_security_audit_tools()
            .with_gadget_chain_tools()
            .with_auto_gadget_tools()
            .with_attack_surface_tools()
            .with_protocol_check_tools()
            .with_access_control_tools()
            .with_run_poc_tools();
        if internal_names.contains("apply_patch") {
            builder = builder.with_patch_tools();
        }

        let mut context = ToolContext::new(workspace.clone());
        // Local execution backend for `run_poc`: the OS sandbox
        // (Landlock/seatbelt preparation + local process exec) implements the
        // SandboxBackend seam, so DAST works with no container/runtime config.
        context.sandbox_backend = Some(Arc::new(crate::sandbox::OsSandbox::default()));
        let registry = builder.build(context);

        // Loop v4 / T5: trajectory sink so external MCP tool calls land in a
        // replayable session.jsonl (visible via `mimofan export-session`).
        // Best-effort: open a raw (unredacted) sink so the driving harness can
        // score local-tool results; failure just disables recording.
        let trajectory = open_external_trajectory();

        Ok(Self {
            workspace,
            registry,
            exposed_tools,
            require_approval: settings.require_approval,
            threads: Arc::new(Mutex::new(HashMap::new())),
            next_notification_id: 0,
            backend,
            trajectory,
        })
    }

    fn run(&mut self) -> Result<()> {
        let runtime = Runtime::new().context("Failed to start MCP runtime")?;
        let stdin = io::stdin();
        let mut stdout = io::stdout();

        for line in stdin.lock().lines() {
            let line = line?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let Ok(message) = serde_json::from_str::<Value>(trimmed) else {
                continue;
            };

            if let Some(response) = self.handle_message(&runtime, message) {
                let payload = serde_json::to_string(&response)?;
                writeln!(stdout, "{payload}")?;
                stdout.flush()?;
            }
        }

        Ok(())
    }

    fn handle_message(&mut self, runtime: &Runtime, message: Value) -> Option<Value> {
        let method = message.get("method").and_then(Value::as_str)?;
        let id = message.get("id").cloned();

        match method {
            "initialize" => respond(id.as_ref(), initialize_response()),
            "tools/list" => respond(id.as_ref(), self.list_tools_response()),
            "tools/call" => {
                let params = message.get("params").cloned().unwrap_or_else(|| json!({}));
                match self.call_tool(runtime, params, id.clone()) {
                    Ok(result) => respond(id.as_ref(), result),
                    Err(err) => respond_error(id.as_ref(), err.code, err.message),
                }
            }
            "resources/list" => respond(id.as_ref(), self.list_resources_response()),
            "ping" => respond(id.as_ref(), json!({})),
            "notifications/initialized" => None,
            _ => respond_error(id.as_ref(), -32601, format!("Method not found: {method}")),
        }
    }

    fn list_tools_response(&self) -> Value {
        let mut tools = Vec::new();
        let mut seen = HashSet::new();
        for entry in &self.exposed_tools {
            if !seen.insert(entry.public.clone()) {
                continue;
            }
            match entry.internal.as_str() {
                "deepseek" => {
                    tools.push(json!({
                        "name": "mimofan",
                        "description": "Send a prompt to Mimofan and get a response. Creates a new conversation thread.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "prompt": {
                                    "type": "string",
                                    "description": "The user prompt to send to the API"
                                },
                                "model": {
                                    "type": "string",
                                    "description": "Optional model identifier (default: deepseek-v4-pro)"
                                },
                                "cwd": {
                                    "type": "string",
                                    "description": "Optional working directory context"
                                }
                            },
                            "required": ["prompt"]
                        }
                    }));
                }
                "mimofan-reply" => {
                    tools.push(json!({
                        "name": "mimofan-reply",
                        "description": "Continue an existing conversation thread with Mimofan. Requires a thread_id from a previous mimofan call.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "thread_id": {
                                    "type": "string",
                                    "description": "Thread ID from a previous mimofan call"
                                },
                                "prompt": {
                                    "type": "string",
                                    "description": "The follow-up prompt"
                                },
                                "model": {
                                    "type": "string",
                                    "description": "Optional model override"
                                }
                            },
                            "required": ["thread_id", "prompt"]
                        }
                    }));
                }
                _ => {
                    if let Some(tool) = self.registry.get(&entry.internal) {
                        tools.push(json!({
                            "name": entry.public,
                            "description": tool.description(),
                            "inputSchema": tool.input_schema(),
                        }));
                    }
                }
            }
        }
        json!({ "tools": tools, "nextCursor": Value::Null })
    }

    fn list_resources_response(&self) -> Value {
        let mut resources = Vec::new();
        resources.push(json!({
            "uri": format!("file://{}", self.workspace.display()),
            "name": "workspace",
            "description": "Workspace root",
            "mimeType": "inode/directory",
        }));

        // 经端口列举历史会话（不再直接依赖 session_manager 实现模块）
        for session in self.backend.list_sessions() {
            resources.push(json!({
                "uri": format!("mimofan://session/{}", session.id),
                "name": session.title,
                "description": format!("{} messages", session.message_count),
                "mimeType": "application/json",
            }));
        }

        json!({ "resources": resources, "nextCursor": Value::Null })
    }

    fn call_tool(
        &mut self,
        runtime: &Runtime,
        params: Value,
        request_id: Option<Value>,
    ) -> Result<Value, RpcError> {
        let params = params.as_object().ok_or_else(|| RpcError {
            code: -32602,
            message: "Invalid params for tools/call".to_string(),
        })?;
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| RpcError {
                code: -32602,
                message: "Missing tool name".to_string(),
            })?;

        if self.require_approval
            && !params
                .get("approved")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        {
            return Err(RpcError {
                code: -32001,
                message: "Approval required. Resend with approved=true.".to_string(),
            });
        }

        let internal = self
            .exposed_tools
            .iter()
            .find(|tool| tool.public == name)
            .map(|tool| tool.internal.clone())
            .ok_or_else(|| RpcError {
                code: -32602,
                message: format!("Tool not exposed: {name}"),
            })?;

        // Handle mimofan and mimofan-reply natively
        if internal == "mimofan" || internal == "mimofan-reply" {
            let arguments = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            return self.handle_api_call(runtime, &internal, &arguments, request_id);
        }

        let arguments = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));

        // T5 (G5.3): external MCP tool calls emit to the session trajectory so
        // `mimofan export-session --task mcp-server` shows what the external
        // agent drove locally.
        if let Some(sink) = self.trajectory.as_ref() {
            let mut ev = crate::core::engine::trace::SessionEvent::new(
                crate::core::engine::trace::SessionEventKind::ToolCall,
                0,
                Some("mcp-server".to_string()),
            );
            ev.tool_name = Some(internal.clone());
            ev.tool_input = Some(arguments.clone());
            ev.source = Some("mcp".to_string());
            let _ = sink.emit(&ev);
        }

        let result = runtime.block_on(self.registry.execute_full(&internal, arguments.clone()));

        if let Some(sink) = self.trajectory.as_ref() {
            use crate::core::engine::trace::{SessionEvent, SessionEventKind};
            match &result {
                Ok(tr) => {
                    let mut ev = SessionEvent::new(
                        SessionEventKind::ToolResult,
                        0,
                        Some("mcp-server".into()),
                    );
                    ev.tool_name = Some(internal.clone());
                    ev.tool_result = Some(json!({"success": tr.success, "content": tr.content}));
                    ev.source = Some("mcp".to_string());
                    let _ = sink.emit(&ev);
                }
                Err(e) => {
                    let mut ev =
                        SessionEvent::new(SessionEventKind::Error, 0, Some("mcp-server".into()));
                    ev.tool_name = Some(internal.clone());
                    ev.text = Some(e.to_string());
                    ev.source = Some("mcp".to_string());
                    let _ = sink.emit(&ev);
                }
            }
        }
        Ok(tool_result_to_mcp(result))
    }

    /// Handle a `mimofan` or `mimofan-reply` tool call.
    ///
    /// 经由注入的 `McpBackend` 端口（而非在本模块直接构造 `ApiClient` / `Config`）
    /// 发送 prompt 并返回响应（不走完整 engine）。`mimofan` 新建线程，
    /// `mimofan-reply` 由调用方提供 `thread_id` 续接既有会话。
    fn handle_api_call(
        &mut self,
        runtime: &Runtime,
        internal_name: &str,
        arguments: &Value,
        request_id: Option<Value>,
    ) -> Result<Value, RpcError> {
        let prompt = arguments
            .get("prompt")
            .and_then(Value::as_str)
            .ok_or_else(|| RpcError {
                code: -32602,
                message: "Missing required argument: prompt".to_string(),
            })?;

        let model = arguments
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or("deepseek-v4-pro");

        // Resolve thread_id
        let thread_id = if internal_name == "deepseek" {
            // New thread
            Uuid::new_v4().to_string()
        } else {
            arguments
                .get("thread_id")
                .and_then(Value::as_str)
                .ok_or_else(|| RpcError {
                    code: -32602,
                    message: "Missing required argument: thread_id for mimofan-reply".to_string(),
                })?
                .to_string()
        };

        // 经端口发送聊天请求（加载配置 / 建客户端 / 调 LlmClient 均在端口实现内，
        // 本传输层不再直接依赖 client / config / llm_client 实现模块）

        // Build message list
        let user_message = Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: prompt.to_string(),
                cache_control: None,
            }],
        };

        let messages = if internal_name == "deepseek" {
            vec![user_message]
        } else {
            let thread = self.threads.lock().unwrap_or_else(|e| e.into_inner());
            let mut existing = thread.get(&thread_id).cloned().ok_or_else(|| RpcError {
                code: -32602,
                message: format!("Thread not found: {thread_id}"),
            })?;
            existing.push(user_message);
            existing
        };

        // Send the API request (non-streaming for the basic version)
        let request = MessageRequest {
            model: model.to_string(),
            messages: messages.clone(),
            max_tokens: 16384,
            system: None,
            tools: None,
            tool_choice: None,
            metadata: None,
            thinking: None,
            reasoning_effort: None,
            stream: None,
            temperature: None,
            top_p: None,
            response_format: None,
        };

        let response: MessageResponse = runtime
            .block_on(self.backend.send_message(request))
            .map_err(|e| RpcError {
                code: -32000,
                message: format!("API call failed: {e}"),
            })?;

        // Extract response text from content blocks
        let response_text = response
            .content
            .iter()
            .filter_map(|block| {
                if let ContentBlock::Text { text, .. } = block {
                    Some(text.as_str())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("");

        let usage = &response.usage;

        // Store the assistant response in the thread
        {
            let mut thread = self.threads.lock().unwrap_or_else(|e| e.into_inner());
            let convo = thread.entry(thread_id.clone()).or_default();
            // If mimofan, we already have just the user message; if mimofan-reply,
            // the user message was appended to the cloned messages above but we need
            // to also append it to the stored thread and then the assistant response.
            if internal_name == "deepseek" {
                convo.push(Message {
                    role: "user".to_string(),
                    content: vec![ContentBlock::Text {
                        text: prompt.to_string(),
                        cache_control: None,
                    }],
                });
            }
            convo.push(Message {
                role: "assistant".to_string(),
                content: vec![ContentBlock::Text {
                    text: response_text.clone(),
                    cache_control: None,
                }],
            });
        }

        // Emit a notification/message so the client can correlate the response
        let notification_id = {
            let nid = self.next_notification_id;
            self.next_notification_id += 1;
            nid
        };

        // Write notification to stdout
        let notification = json!({
            "jsonrpc": "2.0",
            "method": "notifications/message",
            "params": {
                "notificationId": notification_id,
                "requestId": request_id,
                "threadId": thread_id,
                "content": response_text,
                "usage": {
                    "inputTokens": usage.input_tokens,
                    "outputTokens": usage.output_tokens,
                }
            }
        });
        if let Ok(payload) = serde_json::to_string(&notification) {
            let mut stdout = io::stdout();
            let _ = writeln!(stdout, "{payload}");
            let _ = stdout.flush();
        }

        Ok(json!({
            "content": [{ "type": "text", "text": &response_text }],
            "isError": false,
            "structuredContent": {
                "threadId": thread_id,
                "content": response_text,
                "usage": {
                    "inputTokens": usage.input_tokens,
                    "outputTokens": usage.output_tokens,
                }
            }
        }))
    }
}

fn default_config_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".mimofan").join("mcp_server.toml"))
}

fn default_expose_tools() -> Vec<String> {
    vec![
        "file_read".to_string(),
        "file_write".to_string(),
        "search".to_string(),
        "apply_patch".to_string(),
        "shell".to_string(),
        "mimofan".to_string(),
        "mimofan-reply".to_string(),
        // T5: SAST/DAST security surface (no SK needed — local tools).
        "security_audit".to_string(),
        "gadget_chain_trace".to_string(),
        "auto_gadget_discovery".to_string(),
        "attack_surface".to_string(),
        "protocol_check".to_string(),
        "access_control".to_string(),
        "run_poc".to_string(),
    ]
}

fn build_exposed_tools(names: &[String]) -> Vec<ExposedTool> {
    let mut tools = Vec::new();
    for name in names {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            continue;
        }
        let public = trimmed.to_string();
        let internal = match trimmed {
            "file_read" => "read_file",
            "file_write" => "write_file",
            "file_edit" => "edit_file",
            "shell" => "exec_shell",
            "search" => "grep_files",
            "file_search" => "file_search",
            // mimofan and mimofan-reply are handled natively in call_tool
            "deepseek" | "mimofan-reply" => trimmed,
            other => other,
        }
        .to_string();
        tools.push(ExposedTool { public, internal });
    }
    tools
}

fn tool_result_to_mcp(result: Result<ToolResult, ToolError>) -> Value {
    match result {
        Ok(tool_result) => {
            let mut response = json!({
                "content": [{ "type": "text", "text": tool_result.content }],
                "isError": !tool_result.success,
            });
            if let Some(metadata) = tool_result.metadata {
                response["structuredContent"] = metadata;
            }
            response
        }
        Err(err) => json!({
            "content": [{ "type": "text", "text": err.to_string() }],
            "isError": true,
        }),
    }
}

fn initialize_response() -> Value {
    json!({
        "protocolVersion": "2024-11-05",
        "serverInfo": {
            "name": "mimofan-mcp-server",
            "version": env!("CARGO_PKG_VERSION"),
        },
        "capabilities": {
            "tools": {},
            "resources": {},
        }
    })
}

fn respond(id: Option<&Value>, result: Value) -> Option<Value> {
    id.map(|id| json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

fn respond_error(id: Option<&Value>, code: i64, message: String) -> Option<Value> {
    id.map(|id| {
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": code, "message": message }
        })
    })
}

#[derive(Debug)]
struct RpcError {
    code: i64,
    message: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn test_server() -> McpServer {
        let settings = McpServerSettings {
            expose_tools: default_expose_tools(),
            require_approval: false,
        };
        let backend: Box<dyn McpBackend> = Box::new(RealMcpBackend);
        // Use a throwaway temp dir as the workspace so file_read targets a
        // known location.
        let workspace =
            std::env::temp_dir().join(format!("mimofan_mcp_test_{}", std::process::id()));
        std::fs::create_dir_all(&workspace).unwrap();
        McpServer::new(workspace, settings, backend).unwrap()
    }

    #[test]
    fn tools_list_includes_file_read() {
        let server = test_server();
        let resp = server.list_tools_response();
        let tools = resp
            .get("tools")
            .and_then(Value::as_array)
            .expect("tools array");
        let names: Vec<&str> = tools
            .iter()
            .filter_map(|t| t.get("name").and_then(Value::as_str))
            .collect();
        assert!(
            names.contains(&"file_read"),
            "tools/list must expose file_read, got {names:?}"
        );
    }

    #[tokio::test]
    async fn tools_call_executes_file_read() {
        let server = test_server();
        let target = server.workspace.join("hello.txt");
        {
            let mut f = std::fs::File::create(&target).unwrap();
            f.write_all(b"reverse-mcp-ok").unwrap();
        }
        let result = server
            .registry
            .execute("read_file", json!({ "path": target.to_string_lossy() }))
            .await
            .expect("read_file executes");
        assert!(
            result.contains("reverse-mcp-ok"),
            "file content returned, got {result}"
        );
    }

    #[test]
    fn tools_list_includes_all_seven_security_tools() {
        // G5.1: security surface exposed over MCP with no SK needed.
        let server = test_server();
        let resp = server.list_tools_response();
        let tools = resp.get("tools").and_then(Value::as_array).unwrap();
        let names: Vec<&str> = tools
            .iter()
            .filter_map(|t| t.get("name").and_then(Value::as_str))
            .collect();
        for required in [
            "security_audit",
            "gadget_chain_trace",
            "auto_gadget_discovery",
            "attack_surface",
            "protocol_check",
            "access_control",
            "run_poc",
        ] {
            assert!(
                names.contains(&required),
                "missing security tool {required} in {names:?}"
            );
        }
    }

    #[test]
    fn run_poc_executes_locally_without_container_or_sk() {
        // G5.2: the local OS sandbox backend makes run_poc return realized.
        // Plain #[test]: call_tool is synchronous and block_ons its own
        // runtime, so it must not be called from inside a tokio context.
        let runtime = Runtime::new().expect("mcp runtime");
        let mut server = test_server();
        let result = server
            .call_tool(
                &runtime,
                json!({
                    "name": "run_poc",
                    "arguments": {
                        "command": "echo poc-realized-marker",
                        "expect": "poc-realized-marker"
                    }
                }),
                Some(json!(42)),
            )
            .expect("run_poc call succeeds");
        let is_error = result
            .get("isError")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        assert!(
            !is_error,
            "run_poc must not error without container: {result}"
        );
        let text = result["content"][0]["text"].as_str().expect("result text");
        let parsed: Value = serde_json::from_str(text).expect("json payload");
        assert_eq!(parsed["realized"], true, "run_poc must be realized: {text}");
    }
}
