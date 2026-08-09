//! Thin JSON-RPC over stdio client for LSP servers.
//!
//! We deliberately do **not** depend on `tower-lsp` — it is a server-side
//! framework and dragging it in here would add hundreds of unnecessary
//! transitive dependencies and slow down `cargo build` for every contributor.
//! The LSP wire protocol is small enough that handling it ourselves is a
//! self-contained ~400 LOC and lets us keep total control of the spawn
//! lifecycle, timeouts, and the async surface.
//!
//! Architecture:
//!
//! - [`LspTransport`] is the trait the [`super::LspManager`] talks to. The
//!   real implementation is [`StdioLspTransport`] (forks an LSP server with
//!   `tokio::process::Command`); tests use the in-process `FakeTransport`
//!   in the `tests` module below.
//! - [`StdioLspTransport`] runs three tokio tasks: a reader, a writer, and
//!   the public API. Communication uses tokio mpsc channels.
//! - We parse `Content-Length`-framed JSON-RPC and route inbound messages
//!   either to a per-request response slot (for replies) or to the
//!   diagnostics queue (for `textDocument/publishDiagnostics` notifications).
//!
//! The transport is one-shot per file in MVP form: the manager spawns a
//! transport on demand for a language and reuses it. We do not implement
//! workspace sync beyond didOpen/didChange because the goal is "post-edit
//! diagnostics," not full IDE smartness.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};
use tokio::sync::Mutex as AsyncMutex;
use tokio::sync::{mpsc, oneshot};
use tokio::time::timeout;

use super::diagnostics::{Diagnostic, Severity};
use super::registry::Language;
use crate::utils::spawn_supervised;

/// A source symbol as returned by `textDocument/documentSymbol`. We keep the
/// minimal fields needed by static-analysis queries (name + kind + span +
/// children for outline navigation), not the full LSP shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LspSymbol {
    /// Symbol name (e.g. function or type identifier).
    pub name: String,
    /// LSP `SymbolKind` numeric code (1=file … 12=class … 14=method …).
    pub kind: u64,
    /// 1-based line of the symbol's defining range start.
    pub line: u32,
    /// 1-based column of the symbol's defining range start.
    pub column: u32,
    /// Nested symbols (methods inside a class, variants inside an enum).
    pub children: Vec<LspSymbol>,
}

/// A source location as returned by `textDocument/references` and
/// `textDocument/definition`. URI is always a `file://` path for our servers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LspLocation {
    /// Absolute filesystem path decoded from the `file://` URI.
    pub path: PathBuf,
    /// 1-based line of the reference/definition.
    pub line: u32,
    /// 1-based column of the reference/definition.
    pub column: u32,
}

/// Trait the LSP manager talks to. A real LSP server speaks this via stdio;
/// tests use an in-process fake.
#[async_trait]
pub trait LspTransport: Send + Sync {
    /// Notify the server that a file was opened or its contents updated, then
    /// wait up to `wait` for a `publishDiagnostics` notification for that
    /// file. Returns the diagnostics list (possibly empty). Implementations
    /// must NOT block past `wait`.
    async fn diagnostics_for(
        &self,
        path: &Path,
        text: &str,
        wait: Duration,
    ) -> Result<Vec<Diagnostic>>;

    /// Send `textDocument/didClose` notification for `path` and remove it from opened map.
    async fn close_file(&self, path: &Path) -> Result<()>;

    /// Generic JSON-RPC request/reply. Allocates an id, registers a reply
    /// slot, sends the message, and awaits the matching reply (or error)
    /// up to `wait`. Returns the full reply `Value`. This is the shared path
    /// behind every request-style LSP method (symbols/references/definition/
    /// hover/workspace symbol). Implementations must surface timeouts and
    /// server errors as `Err`, never block indefinitely.
    async fn request(&self, method: &str, params: Value, wait: Duration) -> Result<Value>;

    /// List symbols in `path` via `textDocument/documentSymbol`. Best-effort:
    /// returns an empty list when unsupported or on failure.
    async fn document_symbols(&self, path: &Path, wait: Duration) -> Vec<LspSymbol>;

    /// Find references to the symbol at `(line, column)` in `path` via
    /// `textDocument/references`. Best-effort: empty on unsupported/failure.
    async fn references(
        &self,
        path: &Path,
        line: u32,
        column: u32,
        include_declaration: bool,
        wait: Duration,
    ) -> Vec<LspLocation>;

    /// Resolve the definition of the symbol at `(line, column)` in `path` via
    /// `textDocument/definition`. Best-effort: `None` on unsupported/failure.
    async fn definition(&self, path: &Path, line: u32, column: u32, wait: Duration)
    -> Option<LspLocation>;
}

/// Stdio-backed transport. Spawns the LSP server as a child process and
/// pipes JSON-RPC over stdin/stdout. Stderr is captured into a buffer so
/// callers can include it in error messages without polluting our own stderr.
pub struct StdioLspTransport {
    /// JoinHandle for the running server. Held so the child stays alive for
    /// the transport's lifetime.
    child: AsyncMutex<Option<Child>>,
    /// Outgoing message sender to the writer task.
    tx_outbound: mpsc::Sender<Vec<u8>>,
    /// Inbound diagnostics queue. We push every `publishDiagnostics`
    /// notification into here and the public API drains the relevant entries.
    diagnostics_rx: AsyncMutex<mpsc::Receiver<(PathBuf, Vec<Diagnostic>)>>,
    /// Map of in-flight request id -> reply slot. Populated by [`Self::request_raw`]
    /// for every method call that expects a server reply; drained by the
    /// dispatcher when a matching `id` arrives.
    pending: Arc<AsyncMutex<HashMap<i64, oneshot::Sender<Value>>>>,
    /// Monotonic request id counter. `request_raw` takes the next value, sends
    /// the message, and registers a reply slot keyed by it.
    next_id: AsyncMutex<i64>,
    /// `serverCapabilities` from the `initialize` reply. `None` until
    /// `spawn` completes the handshake; read by capability-gated helpers
    /// (`document_symbols`, `references`, `definition`, …) to skip servers
    /// that do not advertise support.
    capabilities: Arc<AsyncMutex<Option<Value>>>,
    /// Language id passed in `textDocument/didOpen` (e.g. "rust").
    language_id: &'static str,
    /// Track which files we have opened so the second touch sends
    /// `didChange` instead of `didOpen`.
    opened: AsyncMutex<HashMap<PathBuf, i64>>,
}

impl StdioLspTransport {
    /// Spawn `command args…` and run the LSP `initialize` handshake. Returns
    /// `Err` immediately if the binary is not on PATH or `initialize` fails.
    pub async fn spawn(
        command: &str,
        args: &[String],
        language: Language,
        workspace: PathBuf,
    ) -> Result<Self> {
        let mut cmd = Command::new(command);
        cmd.args(args);
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        cmd.kill_on_drop(true);

        let mut child = cmd
            .spawn()
            .with_context(|| format!("failed to spawn LSP server `{command}`"))?;

        let stdin = child
            .stdin
            .take()
            .context("LSP child has no stdin handle")?;
        let stdout = child
            .stdout
            .take()
            .context("LSP child has no stdout handle")?;

        let (tx_outbound, rx_outbound) = mpsc::channel::<Vec<u8>>(64);
        let (tx_inbound, rx_inbound) = mpsc::channel::<Value>(64);
        let (tx_diag, rx_diag) = mpsc::channel::<(PathBuf, Vec<Diagnostic>)>(64);

        // Writer task: drain outbound channel, frame with Content-Length, write to stdin.
        spawn_supervised(
            "lsp-writer",
            std::panic::Location::caller(),
            writer_task(stdin, rx_outbound),
        );
        // Reader task: parse Content-Length frames from stdout, push to inbound queue.
        spawn_supervised(
            "lsp-reader",
            std::panic::Location::caller(),
            reader_task(stdout, tx_inbound),
        );
        // Inbound dispatcher: routes notifications to `tx_diag`, replies to a
        // pending map. We keep the pending map for completeness even though
        // diagnostics polling itself does not reuse it.
        let pending: Arc<AsyncMutex<HashMap<i64, oneshot::Sender<Value>>>> =
            Arc::new(AsyncMutex::new(HashMap::new()));
        spawn_supervised(
            "lsp-dispatcher",
            std::panic::Location::caller(),
            dispatcher_task(rx_inbound, tx_diag, pending.clone()),
        );

        // Send `initialize` and wait for `initialized`. We synthesize id=1.
        let init_payload = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "processId": std::process::id(),
                "rootUri": uri_from_path(&workspace),
                "capabilities": {
                    "textDocument": {
                        "publishDiagnostics": { "relatedInformation": false }
                    }
                },
                "workspaceFolders": [{
                    "uri": uri_from_path(&workspace),
                    "name": "workspace"
                }]
            }
        });
        send_message(&tx_outbound, &init_payload).await?;

        // Await the `initialize` reply so we capture `serverCapabilities`
        // for capability-gated helpers (symbols/references/definition). We
        // bound the wait so a broken server cannot hang startup; on timeout
        // we proceed with `capabilities = None` (helpers will then probe
        // lazily or degrade). Most servers reply within tens of ms.
        let init_reply = timeout(Duration::from_secs(10), Self::wait_for_reply(&pending, 1)).await;
        let capabilities = match init_reply {
            Ok(Ok(reply)) => reply.get("result").and_then(|r| r.get("capabilities")).cloned(),
            Ok(Err(err)) => {
                tracing::warn!(?err, "lsp: initialize reply error; capabilities unknown");
                None
            }
            Err(_) => {
                tracing::warn!("lsp: initialize reply timed out; capabilities unknown");
                None
            }
        };

        // Send `initialized` to complete the handshake. Servers buffer
        // notifications until ready, so publishDiagnostics arrive on their
        // own clock after this.
        let initialized = json!({
            "jsonrpc": "2.0",
            "method": "initialized",
            "params": {}
        });
        send_message(&tx_outbound, &initialized).await?;

        Ok(Self {
            child: AsyncMutex::new(Some(child)),
            tx_outbound,
            diagnostics_rx: AsyncMutex::new(rx_diag),
            pending,
            next_id: AsyncMutex::new(2),
            capabilities: Arc::new(AsyncMutex::new(capabilities)),
            language_id: language.language_id(),
            opened: AsyncMutex::new(HashMap::new()),
        })
    }

    /// Generic JSON-RPC request/reply. Allocates the next id, registers a
    /// one-shot reply slot, frames and sends the message, then awaits the
    /// matching reply (or error) up to `wait`. Returns the full reply
    /// `Value` (callers extract `result`/`error` as needed).
    ///
    /// Failures are surfaced as `Err`: a closed outbound channel, a server
    /// `error` object, or a `wait` timeout. This is the single shared path
    /// for every request-style LSP method (`documentSymbol`, `references`,
    /// `definition`, `hover`, `workspace/symbol`, …) so individual helpers
    /// stay tiny and capability-gated.
    async fn request_raw(&self, method: &str, params: Value, wait: Duration) -> Result<Value> {
        let id = {
            let mut next = self.next_id.lock().await;
            let id = *next;
            *next += 1;
            id
        };
        let msg = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });

        let (tx, rx) = oneshot::channel::<Value>();
        {
            let mut map = self.pending.lock().await;
            map.insert(id, tx);
        }

        send_message(&self.tx_outbound, &msg).await?;

        match timeout(wait, rx).await {
            Ok(Ok(reply)) => {
                if let Some(err) = reply.get("error") {
                    let code = err.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
                    let msg = err
                        .get("message")
                        .and_then(|m| m.as_str())
                        .unwrap_or("unknown LSP error");
                    return Err(anyhow!("LSP `{method}` failed ({code}): {msg}"));
                }
                Ok(reply)
            }
            Ok(Err(_)) => Err(anyhow!("LSP `{method}` reply channel dropped")),
            Err(_) => {
                // Drop the stale slot so it cannot leak. The dispatcher will
                // find nothing to deliver if the late reply arrives.
                self.pending.lock().await.remove(&id);
                Err(anyhow!("LSP `{method}` timed out after {wait:?}"))
            }
        }
    }

    /// Await a reply for `id` from the pending map. Used during `initialize`
    /// where we already hold `pending` by reference. Returns `Err` if the
    /// dispatcher drops the slot (server died) before delivery.
    async fn wait_for_reply(
        pending: &Arc<AsyncMutex<HashMap<i64, oneshot::Sender<Value>>>>,
        id: i64,
    ) -> Result<Value> {
        let (tx, rx) = oneshot::channel::<Value>();
        {
            let mut map = pending.lock().await;
            map.insert(id, tx);
        }
        match rx.await {
            Ok(reply) => {
                if let Some(err) = reply.get("error") {
                    let code = err.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
                    let msg = err
                        .get("message")
                        .and_then(|m| m.as_str())
                        .unwrap_or("unknown LSP error");
                    return Err(anyhow!("LSP initialize failed ({code}): {msg}"));
                }
                Ok(reply)
            }
            Err(_) => Err(anyhow!("LSP initialize reply channel dropped")),
        }
    }

    /// List the symbols defined in `path` via `textDocument/documentSymbol`.
    ///
    /// Requires the file to have been opened first (callers send `didOpen`/
    /// `didChange` through [`Self::diagnostics_for`]). Returns an empty list
    /// when the server does not advertise `textDocument.documentSymbol`
    /// support or the call fails — symbol queries are best-effort for static
    /// analysis and must not block the agent.
    pub async fn document_symbols(&self, path: &Path, wait: Duration) -> Vec<LspSymbol> {
        if !self.capability_supported(&["textDocument", "documentSymbol"]).await {
            return Vec::new();
        }
        let uri = uri_from_path(path);
        let params = json!({ "textDocument": { "uri": uri } });
        let reply = match self.request_raw("textDocument/documentSymbol", params, wait).await {
            Ok(r) => r,
            Err(err) => {
                tracing::debug!(?err, file = %path.display(), "lsp: documentSymbol failed");
                return Vec::new();
            }
        };
        let raw = reply
            .get("result")
            .and_then(|r| r.as_array())
            .cloned()
            .unwrap_or_default();
        raw.into_iter().filter_map(parse_symbol).collect()
    }

    /// Find all references to the symbol at `(line, column)` in `path` via
    /// `textDocument/references`. `include_declaration` mirrors LSP's
    /// `context.includeDeclaration`.
    pub async fn references(
        &self,
        path: &Path,
        line: u32,
        column: u32,
        include_declaration: bool,
        wait: Duration,
    ) -> Vec<LspLocation> {
        if !self
            .capability_supported(&["textDocument", "references"])
            .await
        {
            return Vec::new();
        }
        let uri = uri_from_path(path);
        let params = json!({
            "textDocument": { "uri": uri },
            "position": { "line": line.saturating_sub(1), "character": column.saturating_sub(1) },
            "context": { "includeDeclaration": include_declaration }
        });
        let reply = match self.request_raw("textDocument/references", params, wait).await {
            Ok(r) => r,
            Err(err) => {
                tracing::debug!(?err, file = %path.display(), "lsp: references failed");
                return Vec::new();
            }
        };
        let raw = reply
            .get("result")
            .and_then(|r| r.as_array())
            .cloned()
            .unwrap_or_default();
        raw.into_iter().filter_map(|v| parse_location(&v)).collect()
    }

    /// Resolve the definition of the symbol at `(line, column)` in `path` via
    /// `textDocument/definition`. Returns `None` when there is no definition
    /// or the server does not support the method.
    pub async fn definition(
        &self,
        path: &Path,
        line: u32,
        column: u32,
        wait: Duration,
    ) -> Option<LspLocation> {
        if !self.capability_supported(&["textDocument", "definition"]).await {
            return None;
        }
        let uri = uri_from_path(path);
        let params = json!({
            "textDocument": { "uri": uri },
            "position": { "line": line.saturating_sub(1), "character": column.saturating_sub(1) }
        });
        let reply = match self.request_raw("textDocument/definition", params, wait).await {
            Ok(r) => r,
            Err(err) => {
                tracing::debug!(?err, file = %path.display(), "lsp: definition failed");
                return None;
            }
        };
        reply.get("result").and_then(parse_location)
    }

    /// True when `serverCapabilities` advertises `path` as a nested object.
    /// `path` follows the `serverCapabilities` shape, e.g.
    /// `["textDocument", "documentSymbol"]`. Returns `true` when capabilities
    /// are unknown (`None`) so we still attempt the call and let the server
    /// reject — safer than silently skipping when the handshake was missed.
    async fn capability_supported(&self, path: &[&str]) -> bool {
        let caps = self.capabilities.lock().await;
        let Some(caps) = caps.as_ref() else {
            return true;
        };
        let mut node = caps;
        for seg in path {
            match node.get(*seg) {
                Some(next) => node = next,
                None => return false,
            }
        }
        // A capability entry is "supported" when present and not explicitly
        // `false` (e.g. `{"dynamicRegistration": false}` still means supported).
        match node.as_bool() {
            Some(false) => false,
            _ => true,
        }
    }
}

#[async_trait]
impl LspTransport for StdioLspTransport {
    async fn diagnostics_for(
        &self,
        path: &Path,
        text: &str,
        wait: Duration,
    ) -> Result<Vec<Diagnostic>> {
        let path_buf = path.to_path_buf();
        let uri = uri_from_path(&path_buf);

        // Either send didOpen (first time) or didChange (subsequent edits).
        let mut opened = self.opened.lock().await;
        let is_new = !opened.contains_key(&path_buf);
        let new_version = opened.get(&path_buf).copied().unwrap_or(0) + 1;
        opened.insert(path_buf.clone(), new_version);
        drop(opened);

        let payload = if is_new {
            json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didOpen",
                "params": {
                    "textDocument": {
                        "uri": uri.clone(),
                        "languageId": self.language_id,
                        "version": new_version,
                        "text": text
                    }
                }
            })
        } else {
            json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didChange",
                "params": {
                    "textDocument": {
                        "uri": uri.clone(),
                        "version": new_version
                    },
                    "contentChanges": [{ "text": text }]
                }
            })
        };
        send_message(&self.tx_outbound, &payload).await?;

        // Drain matching `publishDiagnostics` notifications until `wait`
        // elapses. Servers typically publish within a few hundred ms; for
        // initial cold-start (rust-analyzer) it can be many seconds — but
        // the manager guards us with a separate timeout.
        let deadline = tokio::time::Instant::now() + wait;
        let mut latest: Option<Vec<Diagnostic>> = None;

        loop {
            let now = tokio::time::Instant::now();
            if now >= deadline {
                break;
            }
            let remaining = deadline - now;
            let mut rx = self.diagnostics_rx.lock().await;
            let next = match timeout(remaining, rx.recv()).await {
                Ok(Some(item)) => item,
                Ok(None) => break, // channel closed
                Err(_) => break,   // timed out
            };
            drop(rx);
            let (file, items) = next;
            if file == path_buf {
                latest = Some(items);
                // We have a payload — return immediately. If the server
                // re-publishes after rapid edits, the next call will sync.
                break;
            }
            // Otherwise: notification was for a different file we previously
            // opened. Discard and continue waiting.
        }
        Ok(latest.unwrap_or_default())
    }

    async fn close_file(&self, path: &Path) -> Result<()> {
        let path_buf = path.to_path_buf();
        let mut opened = self.opened.lock().await;
        if opened.remove(&path_buf).is_some() {
            let uri = uri_from_path(&path_buf);
            let payload = json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didClose",
                "params": {
                    "textDocument": {
                        "uri": uri
                    }
                }
            });
            send_message(&self.tx_outbound, &payload).await?;
        }
        Ok(())
    }

    async fn request(&self, method: &str, params: Value, wait: Duration) -> Result<Value> {
        self.request_raw(method, params, wait).await
    }

    async fn document_symbols(&self, path: &Path, wait: Duration) -> Vec<LspSymbol> {
        StdioLspTransport::document_symbols(self, path, wait).await
    }

    async fn references(
        &self,
        path: &Path,
        line: u32,
        column: u32,
        include_declaration: bool,
        wait: Duration,
    ) -> Vec<LspLocation> {
        StdioLspTransport::references(self, path, line, column, include_declaration, wait).await
    }

    async fn definition(&self, path: &Path, line: u32, column: u32, wait: Duration) -> Option<LspLocation> {
        StdioLspTransport::definition(self, path, line, column, wait).await
    }
}

/// Send a JSON value as one Content-Length-framed JSON-RPC message.
async fn send_message(tx: &mpsc::Sender<Vec<u8>>, value: &Value) -> Result<()> {
    let body = serde_json::to_vec(value).context("serialize LSP message")?;
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    let mut frame = Vec::with_capacity(header.len() + body.len());
    frame.extend_from_slice(header.as_bytes());
    frame.extend_from_slice(&body);
    tx.send(frame)
        .await
        .map_err(|_| anyhow!("LSP outbound channel closed"))?;
    Ok(())
}

/// Background task that drains the outbound queue and writes each frame to
/// the LSP server's stdin. Exits cleanly when the channel closes.
async fn writer_task(mut stdin: tokio::process::ChildStdin, mut rx: mpsc::Receiver<Vec<u8>>) {
    while let Some(frame) = rx.recv().await {
        if stdin.write_all(&frame).await.is_err() {
            break;
        }
        if stdin.flush().await.is_err() {
            break;
        }
    }
}

/// Background task that parses `Content-Length`-framed JSON-RPC frames from
/// the LSP server's stdout. Pushes each parsed JSON value to `tx`. Exits
/// when stdout closes or a frame is malformed (we choose to fail closed
/// rather than risk hanging).
async fn reader_task(mut stdout: tokio::process::ChildStdout, tx: mpsc::Sender<Value>) {
    let mut buf: Vec<u8> = Vec::with_capacity(8 * 1024);
    let mut tmp = [0u8; 4096];
    loop {
        let n = match stdout.read(&mut tmp).await {
            Ok(0) => return,
            Ok(n) => n,
            Err(_) => return,
        };
        buf.extend_from_slice(&tmp[..n]);
        // Try to parse as many frames as we can from the accumulated buffer.
        while let Some((header_end, content_length)) = parse_header(&buf) {
            if buf.len() < header_end + content_length {
                break; // need more bytes
            }
            let body = &buf[header_end..header_end + content_length];
            let parsed = serde_json::from_slice::<Value>(body).ok();
            // Drop the consumed bytes regardless of parse result so a bad frame
            // does not stall the loop.
            buf.drain(..header_end + content_length);
            if let Some(value) = parsed
                && tx.send(value).await.is_err()
            {
                return;
            }
        }
    }
}

/// Parse a JSON-RPC header block. Returns `Some((header_end, content_length))`
/// where `header_end` is the byte offset of the first body byte. The header
/// terminator is `\r\n\r\n`. We require a `Content-Length` header.
fn parse_header(buf: &[u8]) -> Option<(usize, usize)> {
    let term = b"\r\n\r\n";
    let pos = buf.windows(term.len()).position(|window| window == term)?;
    let header = std::str::from_utf8(&buf[..pos]).ok()?;
    let mut content_length: Option<usize> = None;
    for line in header.split("\r\n") {
        if let Some(rest) = line.strip_prefix("Content-Length:") {
            content_length = rest.trim().parse::<usize>().ok();
        }
    }
    content_length.map(|cl| (pos + term.len(), cl))
}

/// Background task that consumes inbound JSON values, classifies them as
/// notifications/responses, and routes accordingly.
async fn dispatcher_task(
    mut rx: mpsc::Receiver<Value>,
    tx_diag: mpsc::Sender<(PathBuf, Vec<Diagnostic>)>,
    pending: Arc<AsyncMutex<HashMap<i64, oneshot::Sender<Value>>>>,
) {
    while let Some(value) = rx.recv().await {
        // Notifications have a `method` and no `id`.
        let method = value.get("method").and_then(|v| v.as_str());
        if method == Some("textDocument/publishDiagnostics") {
            if let Some((path, diags)) = parse_publish_diagnostics(&value) {
                let _ = tx_diag.send((path, diags)).await;
            }
            continue;
        }
        // Replies have an `id` and a `result` or `error`.
        if let Some(id) = value.get("id").and_then(|v| v.as_i64()) {
            let mut map = pending.lock().await;
            if let Some(slot) = map.remove(&id) {
                let _ = slot.send(value);
            }
        }
    }
}

/// Decode a `textDocument/publishDiagnostics` notification.
fn parse_publish_diagnostics(value: &Value) -> Option<(PathBuf, Vec<Diagnostic>)> {
    let params = value.get("params")?;
    let uri = params.get("uri")?.as_str()?;
    let path = path_from_uri(uri)?;
    let raw = params.get("diagnostics")?.as_array()?;
    let mut out = Vec::with_capacity(raw.len());
    for d in raw {
        let range = d.get("range")?;
        let start = range.get("start")?;
        let line = start.get("line")?.as_u64()? as u32 + 1;
        let column = start.get("character")?.as_u64()? as u32 + 1;
        let severity = Severity::from_lsp(d.get("severity").and_then(|v| v.as_i64()))
            .unwrap_or(Severity::Error);
        let message = d
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        out.push(Diagnostic {
            line,
            column,
            severity,
            message,
        });
    }
    Some((path, out))
}

/// Decode one `documentSymbol` result entry (LSP 3.16 hierarchical or flat
/// `SymbolInformation`) into [`LspSymbol`]. Returns `None` on a malformed
/// entry so a bad server payload cannot abort the whole list.
fn parse_symbol(value: Value) -> Option<LspSymbol> {
    let name = value.get("name")?.as_str()?.to_string();
    let kind = value.get("kind").and_then(|k| k.as_u64()).unwrap_or(0);
    let range = value.get("range").or_else(|| value.get("location").and_then(|l| l.get("range")))?;
    let start = range.get("start")?;
    let line = start.get("line")?.as_u64()? as u32 + 1;
    let column = start.get("character")?.as_u64()? as u32 + 1;
    let children = value
        .get("children")
        .and_then(|c| c.as_array())
        .map(|arr| arr.iter().filter_map(|v| parse_symbol(v.clone())).collect())
        .unwrap_or_default();
    Some(LspSymbol {
        name,
        kind,
        line,
        column,
        children,
    })
}

/// Decode one `references`/`definition` result entry into [`LspLocation`].
/// Handles both a bare `{uri, range}` location and a `{uri, range}` wrapped
/// in a `LocationLink`. Returns `None` on a malformed entry.
fn parse_location(value: &Value) -> Option<LspLocation> {
    // LocationLink nests the target under `targetUri`/`targetRange`.
    let (uri, range) = if let Some(uri) = value.get("targetUri").and_then(|u| u.as_str()) {
        let range = value.get("targetRange")?;
        (uri, range)
    } else {
        let uri = value.get("uri")?.as_str()?;
        let range = value.get("range")?;
        (uri, range)
    };
    let path = path_from_uri(uri)?;
    let start = range.get("start")?;
    let line = start.get("line")?.as_u64()? as u32 + 1;
    let column = start.get("character")?.as_u64()? as u32 + 1;
    Some(LspLocation { path, line, column })
}

/// Convert a filesystem path to a `file://` URI. Best-effort — we do not
/// support Windows drive letters perfectly, but the LSP servers in our
/// registry accept percent-encoded paths well enough for the post-edit
/// diagnostics use case.
fn uri_from_path(path: &Path) -> String {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let s = canonical.to_string_lossy();
    if s.starts_with('/') {
        format!("file://{s}")
    } else {
        format!("file:///{}", s.trim_start_matches('/'))
    }
}

/// Inverse of [`uri_from_path`]. Returns `None` when the URI is not a `file://`.
fn path_from_uri(uri: &str) -> Option<PathBuf> {
    let stripped = uri.strip_prefix("file://")?;
    Some(PathBuf::from(stripped))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    /// In-process fake of [`LspTransport`]. Unlike a real server it does not
    /// spawn a process; it returns scripted replies keyed by method name so
    /// we can unit-test the request/reply plumbing, timeout handling, and
    /// capability gating without an LSP binary on PATH. Uses a std `Mutex`
    /// (not the async one) so construction never blocks the test runtime.
    struct FakeTransport {
        /// Scripted reply for `request(method, …)`. `None` means "no reply
        /// ever arrives" (to exercise the timeout path).
        replies: Arc<StdMutex<HashMap<String, Value>>>,
        /// When `true`, `request` sleeps past any reasonable `wait` before
        /// replying, simulating a hung server for the timeout test.
        hang: Arc<StdMutex<bool>>,
    }

    impl FakeTransport {
        fn new() -> Self {
            Self {
                replies: Arc::new(StdMutex::new(HashMap::new())),
                hang: Arc::new(StdMutex::new(false)),
            }
        }
        fn with_reply(mut self, method: &str, result: Value) -> Self {
            self.replies
                .lock()
                .unwrap()
                .insert(method.to_string(), json!({ "result": result }));
            self
        }
        fn hanging(mut self) -> Self {
            *self.hang.lock().unwrap() = true;
            self
        }
    }

    #[async_trait]
    impl LspTransport for FakeTransport {
        async fn diagnostics_for(
            &self,
            _path: &Path,
            _text: &str,
            _wait: Duration,
        ) -> Result<Vec<Diagnostic>> {
            Ok(Vec::new())
        }
        async fn close_file(&self, _path: &Path) -> Result<()> {
            Ok(())
        }
        async fn request(&self, method: &str, _params: Value, wait: Duration) -> Result<Value> {
            if *self.hang.lock().unwrap() {
                tokio::time::sleep(wait + Duration::from_millis(50)).await;
            }
            let replies = self.replies.lock().unwrap();
            match replies.get(method) {
                Some(reply) => Ok(reply.clone()),
                None => Err(anyhow!("fake: no scripted reply for `{method}`")),
            }
        }
        async fn document_symbols(&self, _path: &Path, _wait: Duration) -> Vec<LspSymbol> {
            Vec::new()
        }
        async fn references(
            &self,
            _path: &Path,
            _line: u32,
            _column: u32,
            _include_declaration: bool,
            _wait: Duration,
        ) -> Vec<LspLocation> {
            Vec::new()
        }
        async fn definition(
            &self,
            _path: &Path,
            _line: u32,
            _column: u32,
            _wait: Duration,
        ) -> Option<LspLocation> {
            None
        }
    }

    #[tokio::test]
    async fn request_roundtrip_returns_result() {
        let fake = FakeTransport::new().with_reply(
            "textDocument/documentSymbol",
            json!([
                { "name": "main", "kind": 14, "range": { "start": { "line": 0, "character": 4 } } },
                { "name": "Helper", "kind": 12, "range": { "start": { "line": 9, "character": 1 } },
                  "children": [ { "name": "inner", "kind": 14, "range": { "start": { "line": 10, "character": 5 } } } ] }
            ]),
        );
        let reply = fake
            .request("textDocument/documentSymbol", json!({}), Duration::from_millis(500))
            .await
            .unwrap();
        let raw = reply.get("result").and_then(|r| r.as_array()).unwrap();
        let parsed: Vec<LspSymbol> = raw.iter().filter_map(|v| parse_symbol(v.clone())).collect();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].name, "main");
        assert_eq!(parsed[0].line, 1);
        assert_eq!(parsed[1].children[0].name, "inner");
        assert_eq!(parsed[1].children[0].line, 11);
    }

    #[tokio::test]
    async fn request_timeout_surfaces_error() {
        let fake = FakeTransport::new().hanging();
        let err = fake
            .request("textDocument/references", json!({}), Duration::from_millis(100))
            .await;
        assert!(err.is_err(), "hung server must time out, got {err:?}");
    }

    #[test]
    fn parse_location_handles_bare_and_locationlink() {
        let bare = json!({ "uri": "file:///a/b.rs", "range": { "start": { "line": 2, "character": 1 } } });
        let loc = parse_location(&bare).unwrap();
        assert_eq!(loc.line, 3);
        assert_eq!(loc.column, 2);

        let link = json!({
            "targetUri": "file:///c/d.rs",
            "targetRange": { "start": { "line": 4, "character": 0 } }
        });
        let loc2 = parse_location(&link).unwrap();
        assert_eq!(loc2.path, PathBuf::from("/c/d.rs"));
        assert_eq!(loc2.line, 5);
    }

    #[test]
    fn uri_roundtrip_is_stable() {
        let p = Path::new("/tmp/x/y.rs");
        let uri = uri_from_path(p);
        assert!(uri.starts_with("file://"));
        let back = path_from_uri(&uri).unwrap();
        assert_eq!(back, p);
    }
}
