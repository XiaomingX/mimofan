//! Minimal Agent Client Protocol stdio adapter.
//!
//! This intentionally starts with the ACP baseline: initialize, new session,
//! prompt, and cancel. It keeps stdout protocol-clean for editor clients and
//! routes prompts through the same configured DeepSeek client as one-shot CLI
//! mode.

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{Result, anyhow};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};

use crate::client::ApiClient;
use crate::config::Config;
use crate::llm_client::LlmClient;
use crate::models::{ContentBlock, ImageUrlContent, Message, MessageRequest, SystemPrompt};

const ACP_PROTOCOL_VERSION: u64 = 1;

pub async fn run_acp_server(config: Config, model: String, default_cwd: PathBuf) -> Result<()> {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let mut reader = BufReader::new(stdin).lines();
    let mut writer = tokio::io::BufWriter::new(stdout);
    let mut server = AcpServer::new(config, model, default_cwd);

    while let Some(line) = reader.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }

        let message: Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(err) => {
                write_jsonrpc_error(&mut writer, None, -32700, format!("invalid json: {err}"))
                    .await?;
                continue;
            }
        };

        if message.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            write_jsonrpc_error(
                &mut writer,
                message.get("id").cloned(),
                -32600,
                "jsonrpc version must be 2.0",
            )
            .await?;
            continue;
        }

        let id = message.get("id").cloned();
        let method = match message.get("method").and_then(Value::as_str) {
            Some(method) => method,
            None => {
                write_jsonrpc_error(&mut writer, id, -32600, "missing method").await?;
                continue;
            }
        };
        let params = message.get("params").cloned().unwrap_or_else(|| json!({}));

        match server.handle_request(method, params, &mut writer).await {
            Ok(AcpDispatch::Response(result)) => {
                if let Some(id) = id {
                    write_jsonrpc_result(&mut writer, id, result).await?;
                }
            }
            Ok(AcpDispatch::Shutdown) => {
                if let Some(id) = id {
                    write_jsonrpc_result(&mut writer, id, json!(null)).await?;
                }
                break;
            }
            Err(err) => {
                write_jsonrpc_error(&mut writer, id, err.code, err.message).await?;
            }
        }
    }

    Ok(())
}

struct AcpServer {
    config: Config,
    model: String,
    default_cwd: PathBuf,
    sessions: HashMap<String, AcpSession>,
}

struct AcpSession {
    cwd: PathBuf,
    messages: Vec<Message>,
    embedded_context: Option<String>,
}

enum AcpDispatch {
    Response(Value),
    Shutdown,
}

#[derive(Debug)]
struct AcpError {
    code: i32,
    message: String,
}

impl AcpServer {
    fn new(config: Config, model: String, default_cwd: PathBuf) -> Self {
        Self {
            config,
            model,
            default_cwd,
            sessions: HashMap::new(),
        }
    }

    async fn handle_request<W>(
        &mut self,
        method: &str,
        params: Value,
        writer: &mut W,
    ) -> std::result::Result<AcpDispatch, AcpError>
    where
        W: AsyncWrite + Unpin,
    {
        match method {
            "initialize" => Ok(AcpDispatch::Response(initialize_result(
                params.get("protocolVersion").and_then(Value::as_u64),
                &self.config,
            ))),
            "session/new" => Ok(AcpDispatch::Response(self.new_session(params)?)),
            "session/list" => Ok(AcpDispatch::Response(self.list_sessions()?)),
            "session/prompt" => {
                self.prompt(params, writer).await?;
                Ok(AcpDispatch::Response(json!({ "stopReason": "end_turn" })))
            }
            "session/cancel" => Ok(AcpDispatch::Response(json!(null))),
            "mcp/list_tools" => Ok(AcpDispatch::Response(self.list_mcp_tools(params))),
            "shutdown" => Ok(AcpDispatch::Shutdown),
            _ => Err(AcpError::method_not_found(method)),
        }
    }

    fn new_session(&mut self, params: Value) -> std::result::Result<Value, AcpError> {
        let cwd = params
            .get("cwd")
            .and_then(Value::as_str)
            .map(PathBuf::from)
            .unwrap_or_else(|| self.default_cwd.clone());
        let session_id = format!("mimofan-{}", uuid::Uuid::new_v4());
        self.sessions.insert(
            session_id.clone(),
            AcpSession {
                cwd,
                messages: Vec::new(),
                embedded_context: None,
            },
        );
        Ok(json!({ "sessionId": session_id }))
    }

    fn list_sessions(&self) -> std::result::Result<Value, AcpError> {
        let sessions = crate::session_manager::SessionManager::default_location()
            .and_then(|m| m.list_sessions())
            .map_err(|err| AcpError::internal(format!("failed to list sessions: {err}")))?
            .into_iter()
            .map(|s| {
                json!({
                    "sessionId": s.id,
                    "title": s.title,
                    "messageCount": s.message_count,
                })
            })
            .collect::<Vec<_>>();
        Ok(json!({ "sessions": sessions }))
    }

    /// Best-effort MCP tool proxy: exposes configured MCP server names and their
    /// declared tools without forcing a live connection inside the ACP stdio loop.
    /// Live tool resolution is deferred to the engine via `tools/call` forwarding
    /// handled by the host, keeping this adapter protocol-clean and non-blocking.
    fn list_mcp_tools(&self, _params: Value) -> Value {
        let discovered = crate::mcp::McpPool::new(crate::mcp::McpConfig::default())
            .server_names()
            .into_iter()
            .map(|name| {
                json!({
                    "server": name,
                    "connected": false,
                    "note": "live tool list resolved on demand by host agent"
                })
            })
            .collect::<Vec<_>>();
        json!({ "servers": discovered })
    }

    async fn prompt<W>(
        &mut self,
        params: Value,
        writer: &mut W,
    ) -> std::result::Result<(), AcpError>
    where
        W: AsyncWrite + Unpin,
    {
        let session_id = params
            .get("sessionId")
            .and_then(Value::as_str)
            .ok_or_else(|| AcpError::invalid_params("sessionId is required"))?
            .to_string();
        let prompt = extract_prompt_text(params.get("prompt"))
            .filter(|text| !text.trim().is_empty())
            .ok_or_else(|| AcpError::invalid_params("prompt must include text content"))?;
        let images = extract_prompt_images(params.get("prompt"));
        let embedded = params
            .get("embeddedContext")
            .or_else(|| params.get("embedded_context"))
            .cloned();

        // Append user message to session history and clone for the LLM call (avoids borrowing self across await)
        let (messages, cwd) = {
            let session = self
                .sessions
                .get_mut(&session_id)
                .ok_or_else(|| AcpError::invalid_params("unknown sessionId"))?;
            let mut content = vec![ContentBlock::Text {
                text: prompt,
                cache_control: None,
            }];
            for image_url in &images {
                content.push(ContentBlock::ImageUrl {
                    image_url: ImageUrlContent {
                        url: image_url.clone(),
                    },
                });
            }
            if let Some(embedded_value) = &embedded {
                if let Some(system_ctx) = embedded_context_to_text(embedded_value) {
                    session.embedded_context = Some(system_ctx.clone());
                    // store embedded context so run_prompt can prepend it
                    content.push(ContentBlock::Text {
                        text: format!("\n\n[embedded context]\n{system_ctx}"),
                        cache_control: None,
                    });
                }
            }
            session.messages.push(Message {
                role: "user".to_string(),
                content,
            });
            (session.messages.clone(), session.cwd.clone())
        };

        let output = self
            .run_prompt(&messages, &cwd)
            .await
            .map_err(|err| AcpError::internal(err.to_string()))?;

        // Append assistant response to session history
        if !output.is_empty() {
            {
                let session = self
                    .sessions
                    .get_mut(&session_id)
                    .ok_or_else(|| AcpError::invalid_params("unknown sessionId"))?;
                session.messages.push(Message {
                    role: "assistant".to_string(),
                    content: vec![ContentBlock::Text {
                        text: output.clone(),
                        cache_control: None,
                    }],
                });
            }

            write_session_update(writer, &session_id, output)
                .await
                .map_err(|err| AcpError::internal(err.to_string()))?;
        }

        Ok(())
    }

    async fn run_prompt(&self, messages: &[Message], cwd: &PathBuf) -> Result<String> {
        let _cwd_guard = ScopedCurrentDir::new(cwd)?;
        let last_user_text = messages
            .iter()
            .rev()
            .find_map(|m| {
                if m.role == "user" {
                    m.content.iter().find_map(|b| match b {
                        ContentBlock::Text { text, .. } => Some(text.as_str()),
                        _ => None,
                    })
                } else {
                    None
                }
            })
            .unwrap_or("");
        let route =
            crate::resolve_cli_auto_route(&self.config, &self.model, last_user_text).await?;
        let execution_config = crate::config_for_cli_route(&self.config, &route);
        let client = ApiClient::new(&execution_config)?;
        let reasoning_effort = route
            .reasoning_effort
            .and_then(|effort| effort.api_value_for_provider(execution_config.api_provider()))
            .map(str::to_string);

        let request = MessageRequest {
            model: route.model,
            messages: messages.to_vec(),
            max_tokens: 4096,
            system: Some(SystemPrompt::Text(
                include_str!("../prompts/acp_coding_assistant.md")
                    .trim()
                    .to_string(),
            )),
            tools: None,
            tool_choice: None,
            metadata: None,
            thinking: None,
            reasoning_effort,
            stream: Some(false),
            temperature: Some(0.2),
            top_p: Some(0.9),
            response_format: None,
        };

        let response = client.create_message(request).await?;
        let mut output = String::new();
        for block in response.content {
            if let ContentBlock::Text { text, .. } = block {
                output.push_str(&text);
            }
        }
        Ok(output)
    }
}

struct ScopedCurrentDir {
    prior: PathBuf,
}

impl ScopedCurrentDir {
    fn new(cwd: &PathBuf) -> Result<Self> {
        let prior = std::env::current_dir()?;
        if cwd.as_os_str().is_empty() {
            return Ok(Self { prior });
        }
        std::env::set_current_dir(cwd)
            .map_err(|err| anyhow!("failed to enter ACP session cwd {}: {err}", cwd.display()))?;
        Ok(Self { prior })
    }
}

impl Drop for ScopedCurrentDir {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.prior);
    }
}

impl AcpError {
    fn invalid_params(message: impl Into<String>) -> Self {
        Self {
            code: -32602,
            message: message.into(),
        }
    }

    fn method_not_found(method: &str) -> Self {
        Self {
            code: -32601,
            message: format!("method not found: {method}"),
        }
    }

    fn internal(message: impl Into<String>) -> Self {
        Self {
            code: -32603,
            message: message.into(),
        }
    }
}

fn initialize_result(client_protocol_version: Option<u64>, config: &Config) -> Value {
    json!({
        "protocolVersion": client_protocol_version
            .map(|version| version.min(ACP_PROTOCOL_VERSION))
            .unwrap_or(ACP_PROTOCOL_VERSION),
        "agentCapabilities": {
            "loadSession": false,
            "promptCapabilities": {
                "image": true,
                "audio": false,
                "embeddedContext": true
            },
            "mcpCapabilities": {
                "http": false,
                "sse": false,
                "toolProxy": true
            },
            "sessionCapabilities": {
                "list": true
            }
        },
        "agentInfo": {
            "name": "mimofan",
            "title": "mimofan",
            "version": env!("CARGO_PKG_VERSION")
        },
        "authMethods": acp_auth_methods(config)
    })
}

fn acp_auth_methods(config: &Config) -> Value {
    let provider = config.api_provider().as_str();
    json!([
        {
            "id": "mimo-terminal-auth",
            "name": "Set mimofan API key",
            "description": format!("Run mimofan's terminal credential setup for the {provider} provider."),
            "type": "terminal",
            "args": ["auth", "set", "--provider", provider],
            "env": {}
        }
    ])
}

fn extract_prompt_text(prompt: Option<&Value>) -> Option<String> {
    match prompt? {
        Value::String(text) => Some(text.clone()),
        Value::Array(blocks) => {
            let parts = blocks
                .iter()
                .filter_map(content_block_text)
                .collect::<Vec<_>>();
            (!parts.is_empty()).then(|| parts.join("\n\n"))
        }
        _ => None,
    }
}

/// Extract image URLs from an ACP prompt payload (string or block array).
/// Supports both raw data URLs and external http(s) file references.
fn extract_prompt_images(prompt: Option<&Value>) -> Vec<String> {
    let Some(prompt) = prompt else {
        return Vec::new();
    };
    let blocks = match prompt {
        Value::Array(blocks) => blocks,
        _ => return Vec::new(),
    };
    blocks
        .iter()
        .filter_map(|block| match block.get("type").and_then(Value::as_str)? {
            "image" => block
                .get("image")
                .or_else(|| block.get("data"))
                .and_then(Value::as_str)
                .map(str::to_string),
            "image_url" => block
                .get("image_url")
                .and_then(|u| u.get("url"))
                .or_else(|| block.get("url"))
                .and_then(Value::as_str)
                .map(str::to_string),
            _ => None,
        })
        .filter(|url| url.starts_with("data:") || url.starts_with("http://") || url.starts_with("https://"))
        .collect()
}

/// Convert an ACP `embeddedContext` payload into a compact text summary suitable
/// for injection into the model context. Handles visibleFiles, openTabs and cursor.
fn embedded_context_to_text(context: &Value) -> Option<String> {
    let obj = context.as_object()?;
    let mut lines = Vec::new();
    if let Some(visible) = obj.get("visibleFiles").and_then(Value::as_array) {
        if !visible.is_empty() {
            lines.push("Visible files:".to_string());
            for f in visible {
                if let Some(s) = f.as_str() {
                    lines.push(format!("- {s}"));
                } else if let Some(p) = f.get("path").and_then(Value::as_str) {
                    lines.push(format!("- {p}"));
                }
            }
        }
    }
    if let Some(tabs) = obj.get("openTabs").and_then(Value::as_array) {
        if !tabs.is_empty() {
            lines.push("Open tabs:".to_string());
            for t in tabs {
                if let Some(s) = t.as_str() {
                    lines.push(format!("- {s}"));
                } else if let Some(p) = t.get("path").and_then(Value::as_str) {
                    lines.push(format!("- {p}"));
                }
            }
        }
    }
    if let Some(cursor) = obj.get("cursor") {
        let cursor_desc = match cursor {
            Value::String(s) => s.clone(),
            Value::Object(o) => {
                let file = o.get("file").and_then(Value::as_str).unwrap_or("");
                let line = o.get("line").and_then(Value::as_u64).unwrap_or(0);
                let col = o.get("character").and_then(Value::as_u64).unwrap_or(0);
                format!("{file}:{line}:{col}")
            }
            _ => String::new(),
        };
        if !cursor_desc.is_empty() {
            lines.push(format!("Cursor: {cursor_desc}"));
        }
    }
    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

fn content_block_text(block: &Value) -> Option<String> {
    match block.get("type").and_then(Value::as_str)? {
        "text" => block
            .get("text")
            .and_then(Value::as_str)
            .map(str::to_string),
        "resource" => resource_text(block),
        "resource_link" | "resourceLink" => resource_link_text(block),
        _ => None,
    }
}

fn resource_text(block: &Value) -> Option<String> {
    let resource = block.get("resource").unwrap_or(block);
    if let Some(text) = resource.get("text").and_then(Value::as_str) {
        return Some(text.to_string());
    }
    resource_link_text(resource)
}

fn resource_link_text(block: &Value) -> Option<String> {
    let uri = block
        .get("uri")
        .or_else(|| block.pointer("/resource/uri"))
        .and_then(Value::as_str)?;
    Some(format!("@{uri}"))
}

async fn write_session_update<W>(writer: &mut W, session_id: &str, text: String) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    let notification = json!({
        "jsonrpc": "2.0",
        "method": "session/update",
        "params": {
            "sessionId": session_id,
            "update": {
                "sessionUpdate": "agent_message_chunk",
                "content": {
                    "type": "text",
                    "text": text
                }
            }
        }
    });
    write_json_line(writer, notification).await
}

async fn write_jsonrpc_result<W>(writer: &mut W, id: Value, result: Value) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    let id = jsonrpc_response_id(id);
    write_json_line(
        writer,
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result
        }),
    )
    .await
}

async fn write_jsonrpc_error<W>(
    writer: &mut W,
    id: Option<Value>,
    code: i32,
    message: impl Into<String>,
) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    let id = id.map(jsonrpc_response_id);
    write_json_line(
        writer,
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {
                "code": code,
                "message": message.into()
            }
        }),
    )
    .await
}

async fn write_json_line<W>(writer: &mut W, value: Value) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    writer.write_all(value.to_string().as_bytes()).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await?;
    Ok(())
}

fn jsonrpc_response_id(id: Value) -> Value {
    match id {
        Value::Null => Value::Null,
        Value::String(_) => id,
        Value::Number(number) => Value::String(number.to_string()),
        other => Value::String(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_prompt_images_parses_blocks_and_filters_non_http() {
        let prompt = json!([
            { "type": "text", "text": "look" },
            { "type": "image", "image": "data:image/png;base64,AAAA" },
            { "type": "image_url", "image_url": { "url": "https://example.com/x.png" } },
            { "type": "image", "image": "/etc/passwd" }
        ]);
        let images = extract_prompt_images(Some(&prompt));
        assert_eq!(images.len(), 2);
        assert!(images.contains(&"data:image/png;base64,AAAA".to_string()));
        assert!(images.contains(&"https://example.com/x.png".to_string()));
    }

    #[test]
    fn embedded_context_renders_visible_files_and_cursor() {
        let ctx = json!({
            "visibleFiles": ["src/main.rs", "src/lib.rs"],
            "openTabs": [{ "path": "README.md" }],
            "cursor": { "file": "src/main.rs", "line": 42, "character": 7 }
        });
        let text = embedded_context_to_text(&ctx).expect("should render");
        assert!(text.contains("src/main.rs"));
        assert!(text.contains("README.md"));
        assert!(text.contains("src/main.rs:42:7"));
    }

    #[test]
    fn embedded_context_returns_none_when_empty() {
        assert!(embedded_context_to_text(&json!({})).is_none());
    }

    #[test]
    fn initialize_declares_image_and_session_list_capabilities() {
        let cfg = Config::load(None, None).expect("load config");
        let result = initialize_result(Some(ACP_PROTOCOL_VERSION), &cfg);
        let caps = result.get("agentCapabilities").expect("caps");
        assert_eq!(
            caps.get("promptCapabilities")
                .and_then(|c| c.get("image"))
                .and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            caps.get("sessionCapabilities")
                .and_then(|c| c.get("list"))
                .and_then(Value::as_bool),
            Some(true)
        );
    }

    #[test]
    fn mcp_list_tools_returns_array_even_with_no_config() {
        let cfg = Config::load(None, None).expect("load config");
        let server = AcpServer::new(cfg, "deepseek".to_string(), PathBuf::from("/tmp"));
        let result = server.list_mcp_tools(json!({}));
        assert!(result.get("servers").is_some());
    }
}
