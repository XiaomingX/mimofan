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

use crate::client::DeepSeekClient;
use crate::config::Config;
use crate::llm_client::LlmClient;
use crate::models::{ContentBlock, Message, MessageRequest, SystemPrompt};

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
            "session/prompt" => {
                self.prompt(params, writer).await?;
                Ok(AcpDispatch::Response(json!({ "stopReason": "end_turn" })))
            }
            "session/cancel" => Ok(AcpDispatch::Response(json!(null))),
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
            },
        );
        Ok(json!({ "sessionId": session_id }))
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

        // Append user message to session history and clone for the LLM call (avoids borrowing self across await)
        let (messages, cwd) = {
            let session = self
                .sessions
                .get_mut(&session_id)
                .ok_or_else(|| AcpError::invalid_params("unknown sessionId"))?;
            session.messages.push(Message {
                role: "user".to_string(),
                content: vec![ContentBlock::Text {
                    text: prompt,
                    cache_control: None,
                }],
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
        let client = DeepSeekClient::new(&execution_config)?;
        let reasoning_effort = route
            .reasoning_effort
            .and_then(|effort| effort.api_value_for_provider(execution_config.api_provider()))
            .map(str::to_string);

        let request = MessageRequest {
            model: route.model,
            messages: messages.to_vec(),
            max_tokens: 4096,
            system: Some(SystemPrompt::Text(
                include_str!("prompts/acp_coding_assistant.md")
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
                "image": false,
                "audio": false,
                "embeddedContext": true
            },
            "mcpCapabilities": {
                "http": false,
                "sse": false
            },
            "sessionCapabilities": {}
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
mod tests {}
