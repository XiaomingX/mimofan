//! MCP Transport implementations
//!
//! This module contains the transport layer implementations for MCP:
//! - StdioTransport: For local MCP servers via stdin/stdout
//! - SseTransport: For remote MCP servers via Server-Sent Events
//! - HttpTransport: For remote MCP servers via HTTP
//! - StreamableHttpTransport: For MCP servers supporting streamable HTTP

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::StatusCode;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::Mutex as TokioMutex;

use super::McpServerConfig;
use super::headers::{apply_safe_custom_headers, with_default_mcp_http_headers};
use super::oauth;

// === Transport Trait ===

#[async_trait::async_trait]
pub trait McpTransport: Send + Sync {
    async fn send(&mut self, msg: Vec<u8>) -> Result<()>;
    async fn recv(&mut self) -> Result<Vec<u8>>;

    /// Graceful shutdown — stdio transports send SIGTERM to the child and
    /// give it a brief window to exit before tokio's `kill_on_drop` fires
    /// SIGKILL as the backstop. Default is a no-op for non-stdio transports
    /// that have no child process. Mimofanscale#420.
    async fn shutdown(&mut self) {}
}

// === Stdio Transport ===

pub struct StdioTransport {
    pub child: Child,
    pub stdin: ChildStdin,
    pub reader: tokio::io::BufReader<ChildStdout>,
    /// Tail of stderr lines from the spawned MCP server. A background task
    /// drains the child's stderr into this buffer so a mid-run crash leaves
    /// some context behind instead of `Stdio::null` swallowing it.
    pub stderr_tail: Arc<StderrTail>,
}

/// How long `StdioTransport::shutdown` waits for the child to exit on SIGTERM
/// before `kill_on_drop` fires SIGKILL. Tuned short so a hung MCP server
/// can't stall TUI exit; well-behaved servers almost always exit within
/// a few hundred ms.
const STDIO_SHUTDOWN_GRACE: Duration = Duration::from_millis(2_000);

/// How many lines of MCP-server stderr to keep around for crash diagnostics.
/// Bounded so a chatty server can't grow this without limit; large enough to
/// catch typical Node/Python startup or panic output.
const STDERR_TAIL_CAPACITY: usize = 64;

/// Bounded ring buffer for the most recent stderr lines from a spawned MCP
/// server. Used by `StdioTransport` to surface server-side context when the
/// transport read side fails (server crashed, exited early, etc).
#[derive(Default)]
pub(super) struct StderrTail {
    lines: TokioMutex<VecDeque<String>>,
}

impl StderrTail {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self {
            lines: TokioMutex::new(VecDeque::with_capacity(STDERR_TAIL_CAPACITY)),
        })
    }

    pub(super) async fn push(&self, line: String) {
        let mut buf = self.lines.lock().await;
        if buf.len() >= STDERR_TAIL_CAPACITY {
            buf.pop_front();
        }
        buf.push_back(line);
    }

    pub(super) async fn snapshot(&self) -> Vec<String> {
        self.lines.lock().await.iter().cloned().collect()
    }
}

/// Format the captured stderr tail for inclusion in an error message. Empty
/// tails return `None` so the caller can fall back to its original message.
async fn format_stderr_context(tail: &StderrTail) -> Option<String> {
    let lines = tail.snapshot().await;
    if lines.is_empty() {
        return None;
    }
    Some(format!(
        "MCP server stderr (last {} line{}):\n{}",
        lines.len(),
        if lines.len() == 1 { "" } else { "s" },
        lines.join("\n"),
    ))
}

/// Best-effort SIGTERM. On Unix uses `libc::kill`; on Windows there's no
/// equivalent so we let `kill_on_drop` (TerminateProcess) handle it via the
/// subsequent Drop. Returns whether a signal was actually sent.
fn send_sigterm(child: &Child) -> bool {
    #[cfg(unix)]
    {
        if let Some(pid) = child.id() {
            // SAFETY: pid was just obtained from `child.id()`. `libc::kill`
            // with `SIGTERM` is async-signal-safe and never observes invalid
            // memory. Worst case (pid wrap / process already gone) returns
            // ESRCH, which we deliberately ignore.
            unsafe {
                let _ = libc::kill(pid as i32, libc::SIGTERM);
            }
            return true;
        }
        false
    }
    #[cfg(not(unix))]
    {
        let _ = child;
        false
    }
}

#[async_trait::async_trait]
impl McpTransport for StdioTransport {
    async fn send(&mut self, mut msg: Vec<u8>) -> Result<()> {
        msg.push(b'\n');
        self.stdin.write_all(&msg).await?;
        self.stdin.flush().await?;
        Ok(())
    }

    async fn recv(&mut self) -> Result<Vec<u8>> {
        let mut line = String::new();
        loop {
            line.clear();
            let bytes = match self.reader.read_line(&mut line).await {
                Ok(b) => b,
                Err(err) => {
                    if let Some(stderr) = format_stderr_context(&self.stderr_tail).await {
                        anyhow::bail!("Stdio transport read error: {err}\n{stderr}");
                    }
                    return Err(err.into());
                }
            };
            if bytes == 0 {
                if let Some(stderr) = format_stderr_context(&self.stderr_tail).await {
                    anyhow::bail!("Stdio transport closed\n{stderr}");
                }
                anyhow::bail!("Stdio transport closed");
            }

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            return Ok(trimmed.as_bytes().to_vec());
        }
    }

    /// Send SIGTERM and wait up to `STDIO_SHUTDOWN_GRACE` for graceful exit
    /// before letting Drop / `kill_on_drop` fire SIGKILL as the backstop.
    async fn shutdown(&mut self) {
        send_sigterm(&self.child);
        // Give the child a window to exit cleanly. Discard the result —
        // either it exits (success) or the timeout fires (Drop will SIGKILL).
        let _ = tokio::time::timeout(STDIO_SHUTDOWN_GRACE, self.child.wait()).await;
    }
}

/// Drop fallback (#420): if `shutdown` was never called explicitly, still
/// fire SIGTERM before tokio's `kill_on_drop` sends SIGKILL. The two
/// signals arrive back-to-back so well-behaved servers at least see the
/// SIGTERM first; misbehaving ones get SIGKILL'd anyway.
impl Drop for StdioTransport {
    fn drop(&mut self) {
        send_sigterm(&self.child);
    }
}

// === HTTP Auth ===

#[derive(Clone, Default)]
pub(super) struct McpHttpAuth {
    headers: HashMap<String, String>,
    env_headers: HashMap<String, String>,
    bearer_token_env_var: Option<String>,
    oauth: Option<oauth::McpOAuthRuntime>,
}

impl McpHttpAuth {
    pub(super) fn from_config(
        config: &McpServerConfig,
        oauth: Option<oauth::McpOAuthRuntime>,
    ) -> Self {
        Self {
            headers: config.headers.clone(),
            env_headers: config.env_headers.clone(),
            bearer_token_env_var: config.bearer_token_env_var.clone(),
            oauth,
        }
    }

    pub(super) async fn resolved_headers(&self) -> Result<HashMap<String, String>> {
        let mut headers = self.headers.clone();
        for (name, env_var) in &self.env_headers {
            if let Ok(value) = std::env::var(env_var)
                && !value.trim().is_empty()
            {
                headers.insert(name.clone(), value);
            }
        }
        if !mcp_headers_have_authorization(&headers)
            && let Some(env_var) = self.bearer_token_env_var.as_deref()
            && let Ok(token) = std::env::var(env_var)
        {
            let token = token.trim();
            if !token.is_empty() {
                headers.insert("Authorization".to_string(), format!("Bearer {token}"));
            }
        }
        if !mcp_headers_have_authorization(&headers)
            && let Some(oauth) = &self.oauth
            && let Some(value) = oauth.authorization_header().await?
        {
            headers.insert("Authorization".to_string(), value);
        }
        Ok(headers)
    }
}

fn mcp_headers_have_authorization(headers: &HashMap<String, String>) -> bool {
    headers
        .keys()
        .any(|key| key.trim().eq_ignore_ascii_case("authorization"))
}

// === SSE Transport ===

pub(super) struct SseTransport {
    client: reqwest::Client,
    base_url: String,
    auth: McpHttpAuth,
    endpoint_url: Option<String>,
    receiver: tokio::sync::mpsc::UnboundedReceiver<SseInbound>,
    pending_messages: VecDeque<Vec<u8>>,
    sse_task: tokio::task::JoinHandle<()>,
}

pub(super) enum SseInbound {
    Endpoint(String),
    Message(Vec<u8>),
}

impl SseTransport {
    pub(super) async fn connect(
        client: reqwest::Client,
        url: String,
        auth: McpHttpAuth,
        cancel_token: tokio_util::sync::CancellationToken,
        endpoint_timeout: Duration,
    ) -> Result<Self> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let client_clone = client.clone();
        let url_clone = url.clone();
        let auth_clone = auth.clone();
        let wait_cancel_token = cancel_token.clone();

        let sse_task = tokio::spawn(async move {
            if cancel_token.is_cancelled() {
                return;
            }
            use futures_util::FutureExt;
            let result = std::panic::AssertUnwindSafe(Self::run_sse_loop(
                client_clone,
                url_clone,
                auth_clone,
                tx,
                cancel_token,
            ))
            .catch_unwind()
            .await;
            match result {
                Ok(res) => {
                    if let Err(e) = res {
                        tracing::error!("SSE loop error: {}", e);
                    }
                }
                Err(panic_err) => {
                    if let Some(msg) = panic_err.downcast_ref::<&str>() {
                        tracing::error!("SSE loop panicked: {}", msg);
                    } else if let Some(msg) = panic_err.downcast_ref::<String>() {
                        tracing::error!("SSE loop panicked: {}", msg);
                    } else {
                        tracing::error!("SSE loop panicked with unknown error");
                    }
                }
            }
        });

        let mut transport = Self {
            client,
            base_url: url,
            auth,
            endpoint_url: None,
            receiver: rx,
            pending_messages: VecDeque::new(),
            sse_task,
        };
        transport
            .wait_for_endpoint(&wait_cancel_token, endpoint_timeout)
            .await?;
        Ok(transport)
    }

    async fn run_sse_loop(
        client: reqwest::Client,
        url: String,
        auth: McpHttpAuth,
        tx: tokio::sync::mpsc::UnboundedSender<SseInbound>,
        cancel_token: tokio_util::sync::CancellationToken,
    ) -> Result<()> {
        let headers = auth.resolved_headers().await?;
        let response = apply_safe_custom_headers(
            with_default_mcp_http_headers(client.get(&url), false),
            &headers,
        )
        .send()
        .await
        .with_context(|| {
            format!(
                "MCP SSE connect failed (transport=http url={})",
                super::mask_url_secrets(&url),
            )
        })?;
        let status = response.status();
        if !status.is_success() {
            let body_excerpt =
                super::bounded_body_excerpt(response, super::ERROR_BODY_PREVIEW_BYTES).await;
            anyhow::bail!(
                "MCP SSE rejected (transport=http url={} status={}): {}",
                super::mask_url_secrets(&url),
                status,
                body_excerpt,
            );
        }

        let mut stream = response.bytes_stream();
        use futures_util::StreamExt;
        let mut buffer = String::new();

        loop {
            if cancel_token.is_cancelled() {
                tracing::debug!("SSE loop cancelled");
                break;
            }
            let item = tokio::select! {
                _ = cancel_token.cancelled() => {
                    tracing::debug!("SSE loop shutting down");
                    break;
                }
                item = stream.next() => {
                    match item {
                        Some(i) => i,
                        None => break,
                    }
                }
            };
            let chunk = item?;
            let s = String::from_utf8_lossy(&chunk);
            buffer.push_str(&s);

            while let Some((pos, separator_len)) = find_sse_event_separator(&buffer) {
                let event_block = buffer[..pos].to_string();
                buffer = buffer[pos + separator_len..].to_string();

                let mut event_type = "message";
                let mut data = String::new();

                for line in event_block.lines() {
                    if let Some(value) = sse_field_value(line, "event:") {
                        event_type = value;
                    } else if let Some(value) = sse_field_value(line, "data:") {
                        if !data.is_empty() {
                            data.push('\n');
                        }
                        data.push_str(value);
                    }
                }

                match event_type {
                    "endpoint" => {
                        let _ = tx.send(SseInbound::Endpoint(data));
                    }
                    "message" if !data.trim().is_empty() => {
                        let _ = tx.send(SseInbound::Message(data.into_bytes()));
                    }
                    other => {
                        tracing::trace!(target: "mcp", event_type = other, "SSE event dropped (heartbeat or unknown)");
                    }
                }
            }
        }
        Ok(())
    }

    async fn wait_for_endpoint(
        &mut self,
        cancel_token: &tokio_util::sync::CancellationToken,
        endpoint_timeout: Duration,
    ) -> Result<()> {
        let timeout = tokio::time::sleep(endpoint_timeout);
        tokio::pin!(timeout);

        loop {
            let msg = tokio::select! {
                _ = cancel_token.cancelled() => {
                    anyhow::bail!("SSE transport cancelled before endpoint was discovered");
                }
                _ = &mut timeout => {
                    anyhow::bail!(
                        "SSE endpoint not received within {}ms",
                        endpoint_timeout.as_millis()
                    );
                }
                msg = self.receiver.recv() => {
                    msg.context("SSE transport closed before endpoint was discovered")?
                }
            };

            match msg {
                SseInbound::Endpoint(endpoint) => {
                    self.store_endpoint(&endpoint)?;
                    return Ok(());
                }
                SseInbound::Message(msg) => self.pending_messages.push_back(msg),
            }
        }
    }

    fn store_endpoint(&mut self, endpoint: &str) -> Result<()> {
        self.endpoint_url = Some(Self::resolve_endpoint_url(&self.base_url, endpoint)?);
        Ok(())
    }

    fn resolve_endpoint_url(base_url: &str, endpoint_url: &str) -> Result<String> {
        if endpoint_url.starts_with("http://") || endpoint_url.starts_with("https://") {
            return Ok(endpoint_url.to_string());
        }
        let base = reqwest::Url::parse(base_url)?;
        let joined = base.join(endpoint_url)?;
        Ok(joined.to_string())
    }
}

#[async_trait::async_trait]
impl McpTransport for SseTransport {
    async fn send(&mut self, msg: Vec<u8>) -> Result<()> {
        // For SSE, messages are sent to the endpoint URL via POST
        let endpoint = self
            .endpoint_url
            .as_ref()
            .context("SSE transport not connected")?
            .clone();

        let headers = self.auth.resolved_headers().await?;
        let response = apply_safe_custom_headers(
            with_default_mcp_http_headers(self.client.post(&endpoint), false),
            &headers,
        )
        .header("Content-Type", "application/json")
        .body(msg)
        .send()
        .await
        .with_context(|| {
            format!(
                "MCP SSE send failed (url={})",
                super::mask_url_secrets(&endpoint),
            )
        })?;

        let status = response.status();
        if !status.is_success() {
            let body_excerpt =
                super::bounded_body_excerpt(response, super::ERROR_BODY_PREVIEW_BYTES).await;
            anyhow::bail!(
                "MCP SSE send rejected (url={} status={}): {}",
                super::mask_url_secrets(&endpoint),
                status,
                body_excerpt,
            );
        }

        Ok(())
    }

    async fn recv(&mut self) -> Result<Vec<u8>> {
        // Check if we have pending messages from the SSE loop
        if let Some(msg) = self.pending_messages.pop_front() {
            return Ok(msg);
        }

        // Otherwise wait for the next message from the SSE loop
        match self.receiver.recv().await {
            Some(SseInbound::Message(msg)) => Ok(msg),
            Some(SseInbound::Endpoint(endpoint)) => {
                // If we get another endpoint, update and try again
                self.store_endpoint(&endpoint)?;
                self.recv().await
            }
            None => anyhow::bail!("SSE transport closed"),
        }
    }

    async fn shutdown(&mut self) {
        self.sse_task.abort();
    }
}

// === HTTP Transport ===

pub(super) struct HttpTransport {
    mode: HttpTransportMode,
    client: reqwest::Client,
    base_url: String,
    auth: McpHttpAuth,
    cancel_token: tokio_util::sync::CancellationToken,
    endpoint_timeout: Duration,
}

enum HttpTransportMode {
    Streamable(StreamableHttpTransport),
    Sse(SseTransport),
}

struct StreamableHttpTransport {
    client: reqwest::Client,
    url: String,
    /// Request-time auth and custom header resolver for outbound POSTs.
    auth: McpHttpAuth,
    pending_messages: VecDeque<Vec<u8>>,
    /// Per-spec MCP session identifier returned by the server in the
    /// first response (typically the `initialize` response). Attached
    /// as the `Mcp-Session-Id` header on every subsequent outbound
    /// request so the server can correlate messages within the same
    /// session.
    session_id: Option<String>,
}

#[derive(Debug)]
enum StreamableSendError {
    Incompatible(String),
    StaleSession(String),
    Other(anyhow::Error),
}

impl std::fmt::Display for StreamableSendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Incompatible(msg) => write!(f, "Incompatible transport: {msg}"),
            Self::StaleSession(msg) => write!(f, "Stale session: {msg}"),
            Self::Other(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for StreamableSendError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Other(err) => Some(err.as_ref()),
            _ => None,
        }
    }
}

impl HttpTransport {
    pub(super) fn new(
        client: reqwest::Client,
        url: String,
        auth: McpHttpAuth,
        cancel_token: tokio_util::sync::CancellationToken,
        endpoint_timeout: Duration,
    ) -> Self {
        Self {
            mode: HttpTransportMode::Streamable(StreamableHttpTransport::new(
                client.clone(),
                url.clone(),
                auth.clone(),
            )),
            client,
            base_url: url,
            auth,
            cancel_token,
            endpoint_timeout,
        }
    }

    /// Attempt to establish a session by sending an initial message (typically
    /// the `initialize` request) and extracting the session ID from the response.
    /// On failure, the error is returned but the transport remains usable —
    /// callers may choose to fall back to SSE.
    pub(super) async fn try_establish_session(&mut self) -> Result<()> {
        if let HttpTransportMode::Streamable(streamable) = &mut self.mode {
            // Send an empty probe to trigger the server's initialize handshake
            // and capture the Mcp-Session-Id header. The actual initialize
            // message will be sent by the caller; this just warms up the
            // session.
            let _ = streamable.send_with_retry(b"{}".to_vec()).await;
        }
        Ok(())
    }

    async fn switch_to_sse_and_send(&mut self, msg: Vec<u8>) -> Result<()> {
        let mut sse = SseTransport::connect(
            self.client.clone(),
            self.base_url.clone(),
            self.auth.clone(),
            self.cancel_token.clone(),
            self.endpoint_timeout,
        )
        .await?;
        sse.send(msg).await?;
        self.mode = HttpTransportMode::Sse(sse);
        Ok(())
    }
}

impl StreamableHttpTransport {
    fn new(client: reqwest::Client, url: String, auth: McpHttpAuth) -> Self {
        Self {
            client,
            url,
            auth,
            pending_messages: VecDeque::new(),
            session_id: None,
        }
    }

    async fn send_with_retry(&mut self, msg: Vec<u8>) -> Result<()> {
        let headers = self.auth.resolved_headers().await?;
        let mut request = apply_safe_custom_headers(
            with_default_mcp_http_headers(self.client.post(&self.url), false),
            &headers,
        )
        .header("Content-Type", "application/json");

        if let Some(session_id) = &self.session_id {
            request = request.header("Mcp-Session-Id", session_id);
        }

        let response = request.body(msg.clone()).send().await.with_context(|| {
            format!(
                "MCP Streamable HTTP send failed (url={})",
                super::mask_url_secrets(&self.url),
            )
        })?;

        let status = response.status();

        // Check for incompatible status (405 Method Not Allowed)
        if status == StatusCode::METHOD_NOT_ALLOWED {
            let body_excerpt =
                super::bounded_body_excerpt(response, super::ERROR_BODY_PREVIEW_BYTES).await;
            return Err(StreamableSendError::Incompatible(format!(
                "Server returned 405 Method Not Allowed: {}",
                body_excerpt,
            ))
            .into());
        }

        // Check for stale session (404 Not Found or 410 Gone)
        if status == StatusCode::NOT_FOUND || status == StatusCode::GONE {
            let body_excerpt =
                super::bounded_body_excerpt(response, super::ERROR_BODY_PREVIEW_BYTES).await;
            return Err(StreamableSendError::StaleSession(format!(
                "Session not found (status={}): {}",
                status, body_excerpt,
            ))
            .into());
        }

        if !status.is_success() {
            let body_excerpt =
                super::bounded_body_excerpt(response, super::ERROR_BODY_PREVIEW_BYTES).await;
            anyhow::bail!(
                "MCP Streamable HTTP rejected (url={} status={}): {}",
                super::mask_url_secrets(&self.url),
                status,
                body_excerpt,
            );
        }

        // Extract session ID from response headers if present
        if let Some(session_id) = response.headers().get("mcp-session-id") {
            if let Ok(id) = session_id.to_str() {
                self.session_id = Some(id.to_string());
            }
        }

        // Check if response is SSE or JSON
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        if content_type.contains("text/event-stream") {
            // Parse SSE response
            let body = response.text().await?;
            let messages = parse_sse_message_data(&body);
            for msg in messages {
                self.pending_messages.push_back(msg);
            }
        } else {
            // JSON response
            let body = response.text().await?;
            if !body.trim().is_empty() {
                self.pending_messages.push_back(body.into_bytes());
            }
        }

        Ok(())
    }
}

#[async_trait::async_trait]
impl McpTransport for HttpTransport {
    async fn send(&mut self, msg: Vec<u8>) -> Result<()> {
        match &mut self.mode {
            HttpTransportMode::Streamable(streamable) => {
                match streamable.send_with_retry(msg.clone()).await {
                    Ok(()) => Ok(()),
                    Err(e) => {
                        if let Some(streamable_err) = e.downcast_ref::<StreamableSendError>() {
                            match streamable_err {
                                StreamableSendError::Incompatible(_) => {
                                    // Fallback to SSE
                                    self.switch_to_sse_and_send(msg).await
                                }
                                StreamableSendError::StaleSession(_) => {
                                    // Reset session and retry
                                    streamable.session_id = None;
                                    streamable.send_with_retry(msg).await
                                }
                                StreamableSendError::Other(e) => Err(anyhow::anyhow!("{}", e)),
                            }
                        } else {
                            Err(e)
                        }
                    }
                }
            }
            HttpTransportMode::Sse(sse) => sse.send(msg).await,
        }
    }

    async fn recv(&mut self) -> Result<Vec<u8>> {
        match &mut self.mode {
            HttpTransportMode::Streamable(streamable) => {
                if let Some(msg) = streamable.pending_messages.pop_front() {
                    return Ok(msg);
                }
                // For streamable HTTP, we might need to wait for server-sent messages
                // This is a simplified implementation
                tokio::time::sleep(Duration::from_millis(100)).await;
                streamable
                    .pending_messages
                    .pop_front()
                    .ok_or_else(|| anyhow::anyhow!("No messages available"))
            }
            HttpTransportMode::Sse(sse) => sse.recv().await,
        }
    }

    async fn shutdown(&mut self) {
        match &mut self.mode {
            HttpTransportMode::Streamable(_) => {
                // No persistent connection to close
            }
            HttpTransportMode::Sse(sse) => sse.shutdown().await,
        }
    }
}

pub(super) fn is_legacy_sse_transport(config: &McpServerConfig) -> bool {
    config
        .transport
        .as_deref()
        .map(|transport| transport.trim().eq_ignore_ascii_case("sse"))
        .unwrap_or(false)
}

pub(super) fn is_mcp_stale_session_body(body: &str) -> bool {
    let body = body.to_ascii_lowercase();
    body.contains("session") && (body.contains("expired") || body.contains("invalid"))
}

pub(super) fn is_mcp_stale_session_error(err: &anyhow::Error) -> bool {
    let err = format!("{err:#}");
    let lower_err = err.to_ascii_lowercase();
    err.contains("MCP Streamable HTTP session expired")
        || err.contains("MCP session expired")
        || err.contains("SSE transport closed")
        || (err.contains("MCP SSE POST send failed") && is_connection_closed_error_text(&lower_err))
        || is_mcp_stale_session_body(&err)
}

fn is_connection_closed_error_text(err: &str) -> bool {
    err.contains("connection closed")
        || err.contains("connection reset")
        || err.contains("broken pipe")
        || err.contains("unexpected eof")
        || err.contains("forcibly closed")
}

pub(super) fn response_id_matches(id: Option<&serde_json::Value>, expected_id: &str) -> bool {
    let Some(id) = id else {
        return false;
    };
    if id.as_str() == Some(expected_id) {
        return true;
    }
    id.as_u64()
        .map(|id| id.to_string() == expected_id)
        .unwrap_or(false)
}

pub(super) fn validate_mcp_transport(transport: Option<&str>) -> Result<()> {
    let Some(transport) = transport else {
        return Ok(());
    };
    if transport.trim().eq_ignore_ascii_case("sse") {
        return Ok(());
    }
    anyhow::bail!("Unsupported MCP transport '{transport}'. Supported values: sse");
}

pub(super) fn parse_sse_message_data(body: &str) -> Vec<Vec<u8>> {
    let normalized = body.replace("\r\n", "\n");
    let mut messages = Vec::new();

    for block in normalized.split("\n\n") {
        let mut event_type = "message";
        let mut data = String::new();

        for line in block.lines() {
            if let Some(value) = sse_field_value(line, "event:") {
                event_type = value;
            } else if let Some(value) = sse_field_value(line, "data:") {
                if !data.is_empty() {
                    data.push('\n');
                }
                data.push_str(value);
            }
        }

        if event_type != "message" || data.trim().is_empty() {
            continue;
        }

        messages.push(data.trim().as_bytes().to_vec());
    }

    messages
}

pub(super) fn find_sse_event_separator(buffer: &str) -> Option<(usize, usize)> {
    match (buffer.find("\n\n"), buffer.find("\r\n\r\n")) {
        (Some(lf), Some(crlf)) if crlf < lf => Some((crlf, 4)),
        (Some(lf), _) => Some((lf, 2)),
        (_, Some(crlf)) => Some((crlf, 4)),
        _ => None,
    }
}

pub(super) fn sse_field_value<'a>(line: &'a str, field: &str) -> Option<&'a str> {
    let value = line.strip_prefix(field)?;
    Some(value.strip_prefix(' ').unwrap_or(value))
}
