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
use serde_json::Value;
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
pub enum SessionEventKind {
    /// A new agent turn began.
    TurnStart,
    /// Free-form assistant text was produced.
    AssistantText,
    /// A tool was invoked (name + input captured).
    ToolCall,
    /// A tool returned its observation/output (`tool_result`).
    ToolResult,
    /// A sub-agent was dispatched.
    AgentSpawn,
    /// A sub-agent finished.
    AgentDone,
    /// A recoverable or fatal error was recorded.
    Error,
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
    /// prior content is preserved (append-only).
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
        Self::open_at(path)
    }

    /// Open the sink at an arbitrary `path` (creating its parent dir),
    /// appending to any prior content.
    ///
    /// This lets harnesses write a trajectory to a caller-controlled location
    /// instead of always under `~/.mimofan/tasks/<task_id>/`. Reuses the same
    /// `emit` logic as [`SessionEventSink::open`].
    pub fn open_at(path: impl AsRef<Path>) -> Result<Self, std::io::Error> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        debug!(target: "engine::trace", path = %path.display(), "opened SessionEventSink");
        Ok(Self { path })
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
        {
            if let Some(original) = content.as_str()
                && original.chars().count() > MAX_TOOL_OUTPUT_CHARS
            {
                let truncated: String =
                    original.chars().take(MAX_TOOL_OUTPUT_CHARS).collect();
                *content = serde_json::Value::String(format!("{truncated}…"));
                to_write.truncated = Some(true);
            }
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
        match serde_json::from_str::<SessionEvent>(t) {
            Ok(ev) => out.push(ev),
            Err(e) => {
                debug!(target: "engine::trace", line = i, error = %e, "skip malformed session line");
            }
        }
    }
    out
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
        };
        sink.emit(&ev).expect("emit should succeed");

        let read = read_session(&path);
        assert_eq!(read.len(), 1, "open_at round-trip should read one event");
        assert_eq!(read[0].kind, SessionEventKind::ToolCall);
        assert_eq!(read[0].tool_name.as_deref(), Some("bash"));
        assert_eq!(read[0], ev, "structural equality holds");
    }

    #[test]
    fn session_event_old_fields_only_are_backward_compatible() {
        let dir = tempfile::TempDir::new().unwrap();
        let task_id = "backward-compat-test";
        let sink_path = dir.path().join("tasks").join(task_id).join("session.jsonl");
        std::fs::create_dir_all(sink_path.parent().unwrap()).unwrap();

        // A line with ONLY the old fields — no new optional fields present.
        let old_line = r#"{"kind":"AssistantText","turn":1,"ts":"2026-08-15T12:00:00Z","text":"hello"}"#;
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
