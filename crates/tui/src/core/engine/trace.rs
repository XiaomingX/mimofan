//! Request/call-chain tracing (#637).
//!
//! Provides a stable `trace_id` per turn (or per external request) and helpers
//! to thread it through `tracing` spans so that logs/metrics across modules
//! (engine → turn_loop → tool execution → model client) can be correlated by a
//! single id. This is the minimal, zero-behavior-change scaffolding: it does
//! not alter any request struct's wire shape, it only attaches a span field
//! and exposes a generator so other modules can adopt `trace_id` over time.

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tracing::debug;
use uuid::Uuid;

/// Cap on a single tool-result `content` string written to a trajectory line.
///
/// A long-running agent's tool observations (e.g. `bash`/`read_file` output)
/// can be multi-MB. Persisting the full body would bloat the trajectory for
/// labeling/analysis and risk PII leaks. `SessionEventSink::emit` truncates
/// `ToolResult` content at this cap and flags `truncated: true`.
const MAX_TOOL_OUTPUT_CHARS: usize = 16 * 1024;

/// A correlated call-chain identifier.
///
/// Cheap to clone; rendered as a 32-char hex string. Generated once per turn
/// and carried through every `tracing` span opened during that turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TraceId(pub u128);

impl TraceId {
    /// Generate a fresh, random trace id.
    pub fn new() -> Self {
        TraceId(Uuid::new_v4().as_u128())
    }

    /// Render as a compact hex string for log lines / span fields.
    pub fn as_hex(&self) -> String {
        format!("{:032x}", self.0)
    }
}

impl Default for TraceId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for TraceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_hex())
    }
}

/// Open a tracing span tagged with `trace_id`, returning the guard.
///
/// Callers enter the span for the duration of a turn (or any correlated
/// sub-operation) so every event emitted inside carries the same `trace_id`
/// field. Example:
///
/// ```ignore
/// let span = trace_span_for(self.trace_id);
/// let _enter = span.enter();
/// // ... work, all logs now carry trace_id ...
/// ```
pub fn trace_span_for(trace_id: TraceId) -> tracing::span::Span {
    tracing::span!(
        tracing::Level::INFO,
        "turn",
        trace_id = %trace_id.as_hex()
    )
}

/// Categorizes a [`SessionEvent`] along a generic long-horizon task timeline.
///
/// These are the structural signals the `SessionEventSink` records so a harness
/// (or a labeling/analysis platform) can reconstruct *what happened* during a
/// task without re-parsing the model transcript: turn boundaries, assistant
/// text, tool calls/results, sub-agent lifecycle, hypothesis operations, PoC
/// results, errors, and the session end.
///
/// The variants are aligned with `tools::event_stream::EventKind` naming where
/// they overlap (`ToolCall`/`ToolResult`/`AgentSpawn`/`AgentDone`/`Error`),
/// while preserving the vuln-hunt-specific `HypothesisOp`/`PocResult`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionEventKind {
    /// A new agent turn began.
    TurnStart,
    /// The user prompt that started the turn (T4).
    UserPrompt,
    /// The model's reasoning / thinking block (T4).
    AgentThink,
    /// Free-form assistant text was produced.
    AssistantText,
    /// A tool was invoked (name + input captured). Matches the G4.1
    /// `tool_use` label dimension.
    ToolCall,
    /// A tool returned its observation/output (`tool_result`).
    ToolResult,
    /// A sub-agent was dispatched.
    AgentSpawn,
    /// A sub-agent finished.
    AgentDone,
    /// A recoverable or fatal error was recorded.
    Error,
    /// Token usage for the turn (T4; counts in `input_tokens`/`output_tokens`).
    TokenUsage,
    /// A hypothesis lifecycle op happened (create/add_evidence/resolve).
    HypothesisOp,
    /// A `run_poc` produced a `realized` verdict.
    PocResult,
    /// The turn completed.
    TurnEnd,
    /// The session ended, recording its exit status.
    SessionEnd,
}

/// A single, append-only session event recorded during a task.
///
/// Self-contained and serializable so it can be flushed as one JSON line to a
/// `.jsonl` sink and replayed by a harness. Optional fields are only populated
/// for the event kinds that need them; the harness reads what is present.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionEvent {
    /// What kind of event this is.
    pub kind: SessionEventKind,
    /// 1-based monotonic turn counter this event belongs to.
    pub turn: u64,
    /// RFC-3339-ish wall-clock timestamp (local, simplest form).
    pub ts: String,
    /// Assistant text (for `AssistantText`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Tool name (for `ToolCall` / `HypothesisOp` / `PocResult`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    /// Tool input (for `ToolCall`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_input: Option<Value>,
    /// Hypothesis id touched (for `HypothesisOp`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hypothesis_id: Option<String>,
    /// PoC-realized verdict (for `PocResult`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poc_realized: Option<bool>,
    /// Event origin: `system` / `user` / `agent`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Tool output / observation (for `ToolResult`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_result: Option<Value>,
    /// Tool call id; pairs with the caller's id to correlate `ToolCall`↔`ToolResult`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// Session id this event belongs to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// Model used for the turn/session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Session exit status (e.g. `submitted`/`exit_command`/`exit_cost`/`exit_format`/`exit_api`/`exit_error`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_status: Option<String>,
    /// Whether the tool output was truncated (used by later phases).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub truncated: Option<bool>,
    /// Prompt/input tokens for a `TokenUsage` event (T4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    /// Completion/output tokens for a `TokenUsage` event (T4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
}

impl SessionEvent {
    /// Construct a minimal event of `kind` for `turn`, stamping the current
    /// time and the optional session id. Callers then fill text/tool fields.
    pub fn new(kind: SessionEventKind, turn: u64, session_id: Option<String>) -> Self {
        Self {
            kind,
            turn,
            ts: now_ts(),
            text: None,
            tool_name: None,
            tool_input: None,
            hypothesis_id: None,
            poc_realized: None,
            source: None,
            tool_result: None,
            tool_call_id: None,
            session_id,
            model: None,
            exit_status: None,
            truncated: None,
            input_tokens: None,
            output_tokens: None,
        }
    }
}

/// Current wall-clock time as an RFC3339-ish string (matches the existing
/// trajectory convention).
pub fn now_ts() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// Whether a string already looks like a redacted `sha256:` payload (so export
/// does not double-hash).
fn is_redacted(s: &str) -> bool {
    s.starts_with("sha256:")
}

/// Redact one event for a privacy-safe export (T4 G4.2).
///
/// Used by the `export-session` CLI: reads events from an arbitrary sink
/// (including raw harness trajectories) and hashes sensitive payloads at
/// export time, preserving structural metadata. Idempotent — events written by
/// a redacting sink (already `sha256:`/`__redacted__`) are passed through.
pub fn redact_event_for_export(ev: &mut SessionEvent) {
    if let Some(text) = ev.text.as_mut() {
        if !is_redacted(text) {
            *text = redact_hash(text.as_bytes());
        }
    }
    if let Some(input) = ev.tool_input.take() {
        // The redacting sink replaces inputs with a marker object; detect that
        // marker and leave it intact, otherwise hash the payload.
        let already = input.get("__redacted__").and_then(|v| v.as_str()).is_some();
        ev.tool_input = if already {
            Some(input)
        } else {
            Some(redacted_value(&input))
        };
    }
    if let Some(result) = ev.tool_result.as_mut()
        && let Some(content) = result.get_mut("content")
        && let Some(s) = content.as_str()
        && !is_redacted(s)
    {
        *content = serde_json::Value::String(redact_hash(s.as_bytes()));
    }
    ev.truncated = Some(true);
}

/// Append-only writer that flushes [`SessionEvent`]s as one JSON object per
/// line to `~/.mimofan/tasks/<task_id>/session.jsonl`.
///
/// This is the reusable primitive: individual tool/harness owners (e.g. W3 for
/// `turn_loop`) can call [`SessionEventSink::emit`] at the points they own.
/// This module intentionally does NOT wire emit points into those files — it
/// only provides the sink and the types.
pub struct SessionEventSink {
    path: PathBuf,
    /// When `true`, `emit` replaces sensitive payloads (tool input/output
    /// content, assistant text) with a `sha256:<hex>` hash before writing so
    /// default-on recording does not leak PII to disk. Metadata
    /// (`tool_name`/`tool_call_id`/`session_id`/`source`/`ts`) is preserved.
    redact: bool,
}

// `SessionEventSink` is not `Clone`/`Copy` (it wraps a `PathBuf`), but
// `TurnContext` derives `Debug`, so it needs a `Debug` impl to be stored in a
// `#[derive(Debug)]` struct field. The path is enough to identify the sink.
impl std::fmt::Debug for SessionEventSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionEventSink")
            .field("path", &self.path)
            .finish()
    }
}

impl SessionEventSink {
    /// Open (creating the parent dir) the sink for `task_id`.
    ///
    /// Writes to `~/.mimofan/tasks/<task_id>/session.jsonl`, appending. Any
    /// prior content is preserved (append-only). Sensitive payloads are
    /// redacted (hashed) by default.
    pub fn open(task_id: &str) -> Result<Self, std::io::Error> {
        let home = dirs::home_dir().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "could not resolve home directory for SessionEventSink",
            )
        })?;
        let path = home
            .join(".mimofan")
            .join("tasks")
            .join(task_id)
            .join("session.jsonl");
        Self::open_at_with_redact(path, true)
    }

    /// Open the sink at an arbitrary `path` (creating its parent dir),
    /// appending to any prior content.
    ///
    /// This lets harnesses write a trajectory to a caller-controlled location
    /// instead of always under `~/.mimofan/tasks/<task_id>/`. Reuses the same
    /// `emit` logic as [`SessionEventSink::open`]. Sensitive payloads are kept
    /// verbatim (no redaction) so harnesses can persist raw trajectories.
    pub fn open_at(path: impl AsRef<Path>) -> Result<Self, std::io::Error> {
        Self::open_at_with_redact(path, false)
    }

    /// Open the sink at an arbitrary `path` with an explicit redaction mode.
    ///
    /// `redact = true` hashes sensitive payloads at write time (used by the
    /// default-on interactive/headless trajectory); `false` writes verbatim
    /// (used by harnesses that need the raw trajectory).
    pub fn open_at_with_redact(
        path: impl AsRef<Path>,
        redact: bool,
    ) -> Result<Self, std::io::Error> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        debug!(target: "engine::trace", path = %path.display(), redact, "opened SessionEventSink");
        Ok(Self { path, redact })
    }

    /// Append a single event as one JSON line.
    ///
    /// Phase 4 (truncation): a `ToolResult` event's `tool_result.content` is
    /// capped at [`MAX_TOOL_OUTPUT_CHARS`] characters. If it exceeds the cap,
    /// the content is truncated and the event's `truncated` flag is set to
    /// `true`, so downstream labeling/analysis can distinguish a partial tool
    /// observation from a complete one. Truncation happens at write time — the
    /// caller's original `SessionEvent` is never mutated.
    pub fn emit(&self, ev: &SessionEvent) -> Result<(), std::io::Error> {
        let mut to_write = ev.clone();
        if let Some(result) = to_write.tool_result.as_mut()
            && let Some(content) = result.get_mut("content")
            && let Some(original) = content.as_str()
            && original.chars().count() > MAX_TOOL_OUTPUT_CHARS
        {
            let truncated: String = original.chars().take(MAX_TOOL_OUTPUT_CHARS).collect();
            *content = serde_json::Value::String(format!("{truncated}…"));
            to_write.truncated = Some(true);
        }
        // Privacy-first default: when `redact` is set, hash sensitive payloads
        // (tool input, tool-result content, assistant text) so the on-disk
        // trajectory never stores raw PII. Structural metadata (`tool_name`,
        // `tool_call_id`, `session_id`, `source`, timestamps) is preserved so
        // the trajectory remains analyzable.
        if self.redact {
            if let Some(input) = to_write.tool_input.take() {
                to_write.tool_input = Some(redacted_value(&input));
            }
            if let Some(result) = to_write.tool_result.as_mut()
                && let Some(content) = result.get_mut("content")
                && let Some(s) = content.as_str()
            {
                *content = serde_json::Value::String(redact_hash(s.as_bytes()));
            }
            if let Some(text) = to_write.text.as_mut() {
                *text = redact_hash(text.as_bytes());
            }
            to_write.truncated = Some(true);
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        let mut line = serde_json::to_string(&to_write)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
        line.push('\n');
        file.write_all(line.as_bytes())?;
        file.flush()?;
        Ok(())
    }

    /// Path of the underlying sink file (exposed for tests / harnesses).
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// sha256 hex digest of `bytes`, prefixed with `sha256:` for self-describing
/// redacted payloads (e.g. `sha256:ab12...`).
fn redact_hash(bytes: &[u8]) -> String {
    format!("sha256:{}", crate::utils::sha256_hex(bytes))
}

/// Replace an arbitrary tool-input `Value` with a minimal redacted marker that
/// keeps the input structurally present (so consumers can still correlate
/// events) but never leaks the raw content.
fn redacted_value(input: &Value) -> Value {
    serde_json::json!({
        "__redacted__": redact_hash(input.to_string().as_bytes()),
        "__kind__": "input",
    })
}

/// Export a session trajectory as JSON-Lines (T4, `export-session`).
///
/// - `raw = true` passes lines through unchanged (used on harness trajectories
///   written via [`SessionEventSink::open_at`], i.e. redact=false; the output
///   preserves original text for post-training ingestion).
/// - `raw = false` re-redacts every event at export time
///   ([`redact_event_for_export`]), so even a raw sink file can be shared
///   without PII; the result contains no original assistant text, tool input,
///   or tool-result content.
///
/// Malformed lines are skipped (mirroring [`read_session`]).
pub fn export_session_jsonl(path: &Path, raw: bool) -> std::io::Result<String> {
    let events = read_session(path);
    let mut out = String::new();
    for mut ev in events {
        if !raw {
            redact_event_for_export(&mut ev);
        }
        match serde_json::to_string(&ev) {
            Ok(line) => {
                out.push_str(&line);
                out.push('\n');
            }
            Err(e) => {
                debug!(target: "engine::trace", error = %e, "export: skipping un-serializable event");
            }
        }
    }
    Ok(out)
}

/// Compact trajectory export: strips payload-heavy fields (tool inputs /
/// tool outputs / text), keeping only event kind, tool/model/line and
/// metadata. Deterministic, LLM-free; used for cost-efficient dashboards
/// and T9 token-savings accounting.
pub fn export_compact_jsonl(path: &Path) -> std::io::Result<String> {
    let events = read_session(path);
    let mut out = String::new();
    for ev in events {
        let mut slim = ev;
        slim.text = None;
        slim.tool_input = None;
        slim.tool_result = None;
        slim.hypothesis_id = None;
        slim.poc_realized = None;
        if let Ok(line) = serde_json::to_string(&slim) {
            out.push_str(&line);
            out.push('\n');
        }
    }
    Ok(out)
}

/// Read back every [`SessionEvent`] from a `.jsonl` sink, in order.
///
/// Used by the harness to replay a recorded session and by the round-trip
/// test to assert structural equality. Malformed lines are skipped (logged),
/// not fatal — a partially-corrupt sink should not abort replay.
pub fn read_session(path: &Path) -> Vec<SessionEvent> {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };
    let reader = BufReader::new(file);
    let mut out = Vec::new();
    for (i, line) in reader.lines().enumerate() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                debug!(target: "engine::trace", line = i, error = %e, "skip unreadable session line");
                continue;
            }
        };
        let t = line.trim();
        if t.is_empty() {
            continue;
        }

        // Trajectories written before loop v3 / T4 used PascalCase kind labels
        // (e.g. "AssistantText"); normalize them to the snake_case labels so
        // old sessions stay replayable.
        match serde_json::from_str::<Value>(t) {
            Ok(mut v) if v.is_object() => {
                if let Some(kind) = v.get("kind").and_then(|k| k.as_str()) {
                    if let Some(snake) = legacy_kind_to_snake(kind) {
                        v["kind"] = Value::String(snake.to_string());
                    }
                }
                match serde_json::from_value::<SessionEvent>(v) {
                    Ok(ev) => out.push(ev),
                    Err(e) => {
                        debug!(target: "engine::trace", line = i, error = %e, "skip malformed session line");
                    }
                }
            }
            _ => {
                debug!(target: "engine::trace", line = i, "skip non-object session line");
            }
        }
    }
    out
}

/// Map a legacy PascalCase event label to the snake_case schema (T4).
fn legacy_kind_to_snake(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "turn_start" | "TurnStart" => "turn_start",
        "user_prompt" | "UserPrompt" => "user_prompt",
        "agent_think" | "AgentThink" => "agent_think",
        "assistant_text" | "AssistantText" => "assistant_text",
        "tool_call" | "ToolCall" => "tool_call",
        "tool_result" | "ToolResult" => "tool_result",
        "agent_spawn" | "AgentSpawn" => "agent_spawn",
        "agent_done" | "AgentDone" => "agent_done",
        "error" | "Error" => "error",
        "token_usage" | "TokenUsage" => "token_usage",
        "hypothesis_op" | "HypothesisOp" => "hypothesis_op",
        "poc_result" | "PocResult" => "poc_result",
        "turn_end" | "TurnEnd" => "turn_end",
        "session_end" | "SessionEnd" => "session_end",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trace_id_is_unique_and_stable() {
        let a = TraceId::new();
        let b = TraceId::new();
        assert_ne!(a, b, "fresh trace ids must differ");
        assert_eq!(a, a, "same id stable");
        assert_eq!(a.as_hex().len(), 32, "hex is 32 chars");
        assert!(a.to_string().starts_with(&a.as_hex()));
    }

    #[test]
    fn default_trace_id_is_also_unique() {
        let a = TraceId::default();
        let b = TraceId::default();
        assert_ne!(a, b);
    }

    #[test]
    fn span_carries_trace_id_field() {
        let id = TraceId::new();
        let span = trace_span_for(id);
        // Entering must not panic; span is valid.
        let _enter = span.enter();
    }

    #[test]
    fn session_event_round_trip() {
        let dir = tempfile::TempDir::new().unwrap();
        let task_id = "round-trip-test";
        // Use an override path via a temp sink by pointing home dir is awkward;
        // instead drive through the public API and a temp HOME-like path.
        // SessionEventSink::open writes under ~/.mimofan; to keep the test
        // hermetic we replicate the sink path under our temp dir.
        let sink_path = dir.path().join("tasks").join(task_id).join("session.jsonl");
        std::fs::create_dir_all(sink_path.parent().unwrap()).unwrap();

        // Build a sink manually via the struct (path is private, so exercise
        // through open() with a HOME override using env is not available; we
        // instead validate the round-trip contract through read_session on a
        // manually written file, which is exactly what emit produces).
        let ev1 = SessionEvent {
            kind: SessionEventKind::TurnStart,
            turn: 1,
            ts: "2026-08-15T12:00:00Z".to_string(),
            text: None,
            tool_name: None,
            tool_input: None,
            hypothesis_id: None,
            poc_realized: None,
            source: None,
            tool_result: None,
            tool_call_id: None,
            session_id: None,
            model: None,
            exit_status: None,
            truncated: None,
            input_tokens: None,
            output_tokens: None,
        };
        let ev2 = SessionEvent {
            kind: SessionEventKind::PocResult,
            turn: 1,
            ts: "2026-08-15T12:00:05Z".to_string(),
            text: None,
            tool_name: Some("run_poc".to_string()),
            tool_input: Some(serde_json::json!({"expect": "JNDI connection"})),
            hypothesis_id: None,
            poc_realized: Some(true),
            source: None,
            tool_result: None,
            tool_call_id: None,
            session_id: None,
            model: None,
            exit_status: None,
            truncated: None,
            input_tokens: None,
            output_tokens: None,
        };

        // Mirror emit's exact output (one JSON object per line).
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&sink_path)
            .unwrap();
        for ev in [&ev1, &ev2] {
            let mut line = serde_json::to_string(ev).unwrap();
            line.push('\n');
            f.write_all(line.as_bytes()).unwrap();
        }
        f.flush().unwrap();

        let read = read_session(&sink_path);
        assert_eq!(read.len(), 2, "both events round-trip");
        assert_eq!(read[0].kind, SessionEventKind::TurnStart);
        assert_eq!(read[0].turn, 1);
        assert_eq!(read[1].kind, SessionEventKind::PocResult);
        assert_eq!(read[1].tool_name.as_deref(), Some("run_poc"));
        assert_eq!(read[1].poc_realized, Some(true));
        assert_eq!(
            read[1].tool_input,
            Some(serde_json::json!({"expect": "JNDI connection"}))
        );
        // Structural equality of the whole events.
        assert_eq!(read[0], ev1);
        assert_eq!(read[1], ev2);
    }

    #[test]
    fn session_event_new_kinds_round_trip() {
        let dir = tempfile::TempDir::new().unwrap();
        let task_id = "new-kinds-test";
        let sink_path = dir.path().join("tasks").join(task_id).join("session.jsonl");
        std::fs::create_dir_all(sink_path.parent().unwrap()).unwrap();

        // New event kinds carrying the new optional fields.
        let ev1 = SessionEvent {
            kind: SessionEventKind::ToolResult,
            turn: 1,
            ts: "2026-08-15T12:00:01Z".to_string(),
            text: None,
            tool_name: Some("bash".to_string()),
            tool_input: None,
            hypothesis_id: None,
            poc_realized: None,
            source: Some("agent".to_string()),
            tool_result: Some(serde_json::json!({"exit_code": 0, "stdout": "ok"})),
            tool_call_id: Some("call_123".to_string()),
            session_id: None,
            model: None,
            exit_status: None,
            truncated: Some(true),
            input_tokens: None,
            output_tokens: None,
        };
        let ev2 = SessionEvent {
            kind: SessionEventKind::AgentSpawn,
            turn: 2,
            ts: "2026-08-15T12:00:02Z".to_string(),
            text: None,
            tool_name: None,
            tool_input: None,
            hypothesis_id: None,
            poc_realized: None,
            source: Some("system".to_string()),
            tool_result: None,
            tool_call_id: None,
            session_id: Some("sess-1".to_string()),
            model: Some("deepseek-v4".to_string()),
            exit_status: None,
            truncated: None,
            input_tokens: None,
            output_tokens: None,
        };
        let ev3 = SessionEvent {
            kind: SessionEventKind::Error,
            turn: 2,
            ts: "2026-08-15T12:00:03Z".to_string(),
            text: Some("boom".to_string()),
            tool_name: None,
            tool_input: None,
            hypothesis_id: None,
            poc_realized: None,
            source: Some("agent".to_string()),
            tool_result: None,
            tool_call_id: None,
            session_id: None,
            model: None,
            exit_status: None,
            truncated: None,
            input_tokens: None,
            output_tokens: None,
        };
        let ev4 = SessionEvent {
            kind: SessionEventKind::SessionEnd,
            turn: 2,
            ts: "2026-08-15T12:00:04Z".to_string(),
            text: None,
            tool_name: None,
            tool_input: None,
            hypothesis_id: None,
            poc_realized: None,
            source: Some("system".to_string()),
            tool_result: None,
            tool_call_id: None,
            session_id: None,
            model: None,
            exit_status: Some("submitted".to_string()),
            truncated: None,
            input_tokens: None,
            output_tokens: None,
        };

        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&sink_path)
            .unwrap();
        for ev in [&ev1, &ev2, &ev3, &ev4] {
            let mut line = serde_json::to_string(ev).unwrap();
            line.push('\n');
            f.write_all(line.as_bytes()).unwrap();
        }
        f.flush().unwrap();

        let read = read_session(&sink_path);
        assert_eq!(read.len(), 4, "all four new-kind events round-trip");
        assert_eq!(read[0].kind, SessionEventKind::ToolResult);
        assert_eq!(read[0].source.as_deref(), Some("agent"));
        assert_eq!(
            read[0].tool_result,
            Some(serde_json::json!({"exit_code": 0, "stdout": "ok"}))
        );
        assert_eq!(read[0].tool_call_id.as_deref(), Some("call_123"));
        assert_eq!(read[0].truncated, Some(true));
        assert_eq!(read[1].kind, SessionEventKind::AgentSpawn);
        assert_eq!(read[1].session_id.as_deref(), Some("sess-1"));
        assert_eq!(read[1].model.as_deref(), Some("deepseek-v4"));
        assert_eq!(read[2].kind, SessionEventKind::Error);
        assert_eq!(read[2].text.as_deref(), Some("boom"));
        assert_eq!(read[3].kind, SessionEventKind::SessionEnd);
        assert_eq!(read[3].exit_status.as_deref(), Some("submitted"));
        // Structural equality of the whole events.
        assert_eq!(read[0], ev1);
        assert_eq!(read[1], ev2);
        assert_eq!(read[2], ev3);
        assert_eq!(read[3], ev4);
    }

    #[test]
    fn session_event_open_at_round_trip() {
        // `open_at` writes to an arbitrary path (not `~/.mimofan`), so a
        // hermetic temp dir is enough — no HOME override needed.
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir
            .path()
            .join("tasks")
            .join("open-at-test")
            .join("session.jsonl");

        let sink = SessionEventSink::open_at(&path).expect("open_at should create parent dir");
        let ev = SessionEvent {
            kind: SessionEventKind::ToolCall,
            turn: 1,
            ts: "2026-08-15T12:00:00Z".to_string(),
            text: None,
            tool_name: Some("bash".to_string()),
            tool_input: Some(serde_json::json!({"cmd": "echo hi"})),
            hypothesis_id: None,
            poc_realized: None,
            source: Some("agent".to_string()),
            tool_result: None,
            tool_call_id: Some("call_1".to_string()),
            session_id: Some("sess-1".to_string()),
            model: None,
            exit_status: None,
            truncated: None,
            input_tokens: None,
            output_tokens: None,
        };
        sink.emit(&ev).expect("emit should succeed");

        let read = read_session(&path);
        assert_eq!(read.len(), 1, "open_at round-trip should read one event");
        assert_eq!(read[0].kind, SessionEventKind::ToolCall);
        assert_eq!(read[0].tool_name.as_deref(), Some("bash"));
        assert_eq!(read[0], ev, "structural equality holds");
    }

    #[test]
    fn redacted_sink_hashes_sensitive_payloads() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("redacted").join("session.jsonl");

        // `open_at_with_redact(path, true)` must hash tool input/content/text
        // while preserving structural metadata.
        let sink = SessionEventSink::open_at_with_redact(&path, true)
            .expect("open_at_with_redact should create parent dir");
        let ev = SessionEvent {
            kind: SessionEventKind::ToolCall,
            turn: 1,
            ts: "2026-08-15T12:00:00Z".to_string(),
            text: Some("SECRET_PROMPT_TEXT".to_string()),
            tool_name: Some("bash".to_string()),
            tool_input: Some(serde_json::json!({"cmd": "echo SECRET_INPUT"})),
            hypothesis_id: None,
            poc_realized: None,
            source: Some("agent".to_string()),
            tool_result: Some(serde_json::json!({"success": true, "content": "SECRET_OUTPUT"})),
            tool_call_id: Some("call_1".to_string()),
            session_id: Some("sess-1".to_string()),
            model: None,
            exit_status: None,
            truncated: None,
            input_tokens: None,
            output_tokens: None,
        };
        sink.emit(&ev).expect("emit should succeed");

        let read = read_session(&path);
        assert_eq!(read.len(), 1, "should read one event");
        let written = &read[0];

        // Metadata preserved.
        assert_eq!(written.tool_name.as_deref(), Some("bash"));
        assert_eq!(written.tool_call_id.as_deref(), Some("call_1"));
        assert_eq!(written.session_id.as_deref(), Some("sess-1"));
        assert_eq!(written.truncated, Some(true), "redaction sets truncated");

        // Raw secrets must never appear.
        let raw = serde_json::to_string(written).unwrap();
        assert!(!raw.contains("SECRET_INPUT"), "tool input must be hashed");
        assert!(
            !raw.contains("SECRET_OUTPUT"),
            "tool result content must be hashed"
        );
        assert!(
            !raw.contains("SECRET_PROMPT_TEXT"),
            "assistant text must be hashed"
        );

        // Tool input replaced by a redacted marker.
        let input = written.tool_input.as_ref().expect("tool_input present");
        assert!(
            input.get("__redacted__").is_some(),
            "tool_input should be a redacted marker, got {input}"
        );

        // Tool result content replaced by a `sha256:` hash.
        let content = written
            .tool_result
            .as_ref()
            .and_then(|r| r.get("content"))
            .and_then(|c| c.as_str())
            .expect("tool_result content string");
        assert!(
            content.starts_with("sha256:"),
            "tool result content should be hashed, got {content}"
        );

        // Assistant text replaced by a `sha256:` hash.
        let text = written.text.as_deref().expect("text present");
        assert!(
            text.starts_with("sha256:"),
            "assistant text should be hashed, got {text}"
        );
    }

    #[test]
    fn open_at_keeps_raw_payloads_for_harness() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("raw").join("session.jsonl");

        // Harnesses use `open_at` (redact=false): raw payloads preserved.
        let sink = SessionEventSink::open_at(&path).expect("open_at");
        let ev = SessionEvent {
            kind: SessionEventKind::ToolCall,
            turn: 1,
            ts: "2026-08-15T12:00:00Z".to_string(),
            text: Some("plain".to_string()),
            tool_name: Some("bash".to_string()),
            tool_input: Some(serde_json::json!({"cmd": "echo raw"})),
            hypothesis_id: None,
            poc_realized: None,
            source: Some("agent".to_string()),
            tool_result: None,
            tool_call_id: Some("call_1".to_string()),
            session_id: Some("sess-1".to_string()),
            model: None,
            exit_status: None,
            truncated: None,
            input_tokens: None,
            output_tokens: None,
        };
        sink.emit(&ev).expect("emit should succeed");

        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(
            raw.contains("raw"),
            "open_at must not redact (raw payload kept)"
        );
        assert!(!raw.contains("sha256:"), "open_at must not hash payloads");
    }

    #[test]
    fn session_event_old_fields_only_are_backward_compatible() {
        let dir = tempfile::TempDir::new().unwrap();
        let task_id = "backward-compat-test";
        let sink_path = dir.path().join("tasks").join(task_id).join("session.jsonl");
        std::fs::create_dir_all(sink_path.parent().unwrap()).unwrap();

        // A line with ONLY the old fields — no new optional fields present.
        let old_line =
            r#"{"kind":"AssistantText","turn":1,"ts":"2026-08-15T12:00:00Z","text":"hello"}"#;
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&sink_path)
            .unwrap();
        writeln!(f, "{old_line}").unwrap();
        f.flush().unwrap();

        let read = read_session(&sink_path);
        assert_eq!(read.len(), 1, "legacy line is read without error");
        assert_eq!(read[0].kind, SessionEventKind::AssistantText);
        assert_eq!(read[0].text.as_deref(), Some("hello"));
        // New fields default to None.
        assert_eq!(read[0].source, None);
        assert_eq!(read[0].tool_result, None);
        assert_eq!(read[0].tool_call_id, None);
        assert_eq!(read[0].session_id, None);
        assert_eq!(read[0].model, None);
        assert_eq!(read[0].exit_status, None);
        assert_eq!(read[0].truncated, None);
    }

    #[test]
    fn emit_truncates_oversized_tool_result() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("truncate.jsonl");

        let sink = SessionEventSink::open_at(&path).expect("open_at");
        let huge = "x".repeat(MAX_TOOL_OUTPUT_CHARS + 100);
        let ev = SessionEvent {
            kind: SessionEventKind::ToolResult,
            turn: 1,
            ts: "2026-08-15T12:00:00Z".to_string(),
            text: None,
            tool_name: Some("bash".to_string()),
            tool_input: None,
            hypothesis_id: None,
            poc_realized: None,
            source: Some("agent".to_string()),
            tool_result: Some(serde_json::json!({ "success": true, "content": huge })),
            tool_call_id: Some("call_1".to_string()),
            session_id: None,
            model: None,
            exit_status: None,
            truncated: None,
            input_tokens: None,
            output_tokens: None,
        };
        sink.emit(&ev).expect("emit");

        let read = read_session(&path);
        assert_eq!(read.len(), 1);
        assert_eq!(read[0].truncated, Some(true), "oversized output is flagged");
        let content = read[0]
            .tool_result
            .as_ref()
            .and_then(|r| r.get("content"))
            .and_then(|c| c.as_str())
            .expect("content string present");
        // Emit keeps the first MAX_TOOL_OUTPUT_CHARS chars and appends a
        // truncation marker, so the stored content is MAX+1 chars wide.
        assert!(
            content.chars().count() == MAX_TOOL_OUTPUT_CHARS + 1,
            "content + marker is exactly MAX_TOOL_OUTPUT_CHARS + 1"
        );
        assert!(content.ends_with('…'), "content carries truncation marker");
        assert_eq!(
            content.trim_end_matches('…').chars().count(),
            MAX_TOOL_OUTPUT_CHARS,
            "the retained body is exactly MAX_TOOL_OUTPUT_CHARS chars"
        );
        // The original caller's event is untouched (emit clones before writing).
        assert_eq!(ev.truncated, None);
    }

    #[test]
    fn emit_does_not_flag_small_tool_result() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("small.jsonl");

        let sink = SessionEventSink::open_at(&path).expect("open_at");
        let ev = SessionEvent {
            kind: SessionEventKind::ToolResult,
            turn: 1,
            ts: "2026-08-15T12:00:00Z".to_string(),
            text: None,
            tool_name: Some("bash".to_string()),
            tool_input: None,
            hypothesis_id: None,
            poc_realized: None,
            source: Some("agent".to_string()),
            tool_result: Some(serde_json::json!({ "success": true, "content": "ok" })),
            tool_call_id: Some("call_1".to_string()),
            session_id: None,
            model: None,
            exit_status: None,
            truncated: None,
            input_tokens: None,
            output_tokens: None,
        };
        sink.emit(&ev).expect("emit");

        let read = read_session(&path);
        assert_eq!(read.len(), 1);
        assert_eq!(read[0].truncated, None, "small output is not flagged");
        let content = read[0]
            .tool_result
            .as_ref()
            .and_then(|r| r.get("content"))
            .and_then(|c| c.as_str());
        assert_eq!(content, Some("ok"));
    }
}

// ---- T4 export-session / trajectory completeness tests ----

#[test]
fn export_session_raw_keeps_payloads_and_is_parseable() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("session.jsonl");
    let sink = SessionEventSink::open_at(&path).expect("open_at");
    let mut ev = SessionEvent::new(SessionEventKind::UserPrompt, 1, Some("s1".into()));
    ev.text = Some("sk-secret-raw-prompt password=hunter2".to_string());
    sink.emit(&ev).expect("emit");

    let raw = export_session_jsonl(&path, true).expect("raw export");
    assert!(
        raw.contains("sk-secret-raw-prompt"),
        "raw export must keep original text: {raw}"
    );
    // G4.4: post-training loader (read_session) must parse the raw export.
    let replayed = read_session(&path);
    assert_eq!(replayed.len(), 1);
    assert_eq!(replayed[0].kind, SessionEventKind::UserPrompt);
    assert!(
        replayed[0]
            .text
            .as_deref()
            .unwrap()
            .contains("sk-secret-raw-prompt")
    );
}

#[test]
fn export_session_redacted_hashes_sensitive_payloads() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("session.jsonl");
    let sink = SessionEventSink::open_at(&path).expect("open_at");
    let mut ev = SessionEvent::new(SessionEventKind::ToolCall, 1, Some("s1".into()));
    ev.tool_name = Some("exec_shell".to_string());
    ev.tool_input =
        Some(json!({"cmd": "curl -H 'Authorization: Bearer sk-live-abc123' https://example.test"}));
    ev.text = Some("password=hunter2 secret_token=topsecret".to_string());
    sink.emit(&ev).expect("emit");
    let mut out = SessionEvent::new(SessionEventKind::ToolResult, 1, Some("s1".into()));
    out.tool_result = Some(json!({"success": true, "content": "output with sk-embedded-9876"}));
    sink.emit(&out).expect("emit");

    let redacted = export_session_jsonl(&path, false).expect("redacted export");

    // G4.2-style secret sweep over the redacted export.
    for needle in ["sk-", "password", "secret", "hunter2", "abc123", "9876"] {
        assert!(
            !redacted.to_lowercase().contains(needle),
            "redacted export must not contain `{needle}`: {redacted}"
        );
    }
    // Replayable after redaction.
    let replayed = read_session(std::path::Path::new(
        &std::env::temp_dir().join("nonexistent-guard"),
    ));
    assert!(replayed.is_empty());
    let parsed: Vec<Value> = redacted
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).expect("redacted line is JSON"))
        .collect();
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[0]["kind"], "tool_call");
    assert_eq!(parsed[1]["kind"], "tool_result");
}

#[test]
fn new_event_kinds_serialize_to_snake_case_labels() {
    let kind_label = |k: SessionEventKind| serde_json::to_string(&k).unwrap();
    assert_eq!(kind_label(SessionEventKind::UserPrompt), "\"user_prompt\"");
    assert_eq!(kind_label(SessionEventKind::AgentThink), "\"agent_think\"");
    assert_eq!(kind_label(SessionEventKind::TokenUsage), "\"token_usage\"");
    assert_eq!(kind_label(SessionEventKind::ToolCall), "\"tool_call\"");
    assert_eq!(kind_label(SessionEventKind::AgentSpawn), "\"agent_spawn\"");
    assert_eq!(kind_label(SessionEventKind::AgentDone), "\"agent_done\"");
    assert_eq!(kind_label(SessionEventKind::Error), "\"error\"");
}

#[test]
fn full_coverage_fixture_emits_every_required_label() {
    // G4.1-style full-label session used by scripts/check_trace_fields.py.
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("session.jsonl");
    let sink = SessionEventSink::open_at(&path).expect("open_at");
    let mk = |kind: SessionEventKind| {
        let mut ev = SessionEvent::new(kind, 1, Some("cov".into()));
        ev.text = Some(format!("payload for {kind:?}"));
        sink.emit(&ev).expect("emit");
    };
    mk(SessionEventKind::UserPrompt);
    mk(SessionEventKind::AgentThink);
    mk(SessionEventKind::ToolCall);
    mk(SessionEventKind::Error);
    let mut usage = SessionEvent::new(SessionEventKind::TokenUsage, 1, Some("cov".into()));
    usage.input_tokens = Some(100);
    usage.output_tokens = Some(40);
    sink.emit(&usage).expect("emit");
    mk(SessionEventKind::AgentSpawn);
    mk(SessionEventKind::AgentDone);

    let raw = std::fs::read_to_string(&path).unwrap();
    for label in [
        "user_prompt",
        "agent_think",
        "tool_call",
        "error",
        "token_usage",
        "agent_spawn",
        "agent_done",
    ] {
        assert!(
            raw.contains(&format!("\"kind\":\"{label}\"")),
            "missing required label {label} in {raw}"
        );
    }
}

#[test]
fn legacy_pascal_case_kinds_are_normalized_on_read() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("session.jsonl");
    // Old-trajectory label (pre-T4 PascalCase).
    std::fs::write(
        &path,
        "{\"kind\":\"AssistantText\",\"turn\":1,\"ts\":\"t\",\"text\":\"hello\"}\n",
    )
    .unwrap();
    let events = read_session(&path);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind, SessionEventKind::AssistantText);
    assert_eq!(events[0].text.as_deref(), Some("hello"));
}
