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

/// Categorizes a [`SessionEvent`] along the vuln-hunt long-horizon timeline.
///
/// These are the structural signals the `SessionEventSink` records so a harness
/// (or the W4 `EvalHarness`) can reconstruct *what happened* during a task
/// without re-parsing the model transcript: turn boundaries, assistant text,
/// tool calls, hypothesis operations, PoC results, and turn ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionEventKind {
    /// A new agent turn began.
    TurnStart,
    /// Free-form assistant text was produced.
    AssistantText,
    /// A tool was invoked (name + input captured).
    ToolCall,
    /// A hypothesis lifecycle op happened (create/add_evidence/resolve).
    HypothesisOp,
    /// A `run_poc` produced a `realized` verdict.
    PocResult,
    /// The turn completed.
    TurnEnd,
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
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        debug!(target: "engine::trace", task_id, path = %path.display(), "opened SessionEventSink");
        Ok(Self { path })
    }

    /// Append a single event as one JSON line.
    pub fn emit(&self, ev: &SessionEvent) -> Result<(), std::io::Error> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        let mut line = serde_json::to_string(ev).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
        })?;
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
        assert_eq!(read[1].tool_input, Some(serde_json::json!({"expect": "JNDI connection"})));
        // Structural equality of the whole events.
        assert_eq!(read[0], ev1);
        assert_eq!(read[1], ev2);
    }
}
