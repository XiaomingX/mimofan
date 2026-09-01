//! #850 — Structured event stream (jsonl) + replay (READ SIDE ONLY).
//!
//! An append-only, line-delimited JSON log of mimofan's execution. Each entry
//! is an [`EventEnvelope`] carrying a monotonic sequence number, an emission
//! timestamp, a [`EventKind`] tag, and an arbitrary JSON `payload`. The log is
//! meant to be durable and machine-inspectable so a session can be replayed,
//! audited, or turned into metrics without re-running the agent.
//!
//! Loop v1 / T3 normalization: the writer half (`EventLog` / `EventLogError`)
//! was REMOVED. Trajectory emission has a single true source —
//! [`crate::core::engine::trace::SessionEventSink`] (which writes
//! `~/.mimofan/tasks/<id>/session.jsonl`). The headless failure log is also
//! emitted through `SessionEventSink` now. This module keeps only the read /
//! replay data model ([`EventEnvelope`], [`EventKind`], [`replay`],
//! [`EventReplay`], [`EventCounts`]) so external JSONL logs in the envelope
//! schema can still be inspected by the `event_stream` tool. Do not add a new
//! writer here — route emits through `SessionEventSink`.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

/// A single category of structured event.
///
/// The fixed variants cover the core lifecycle of an agent turn and its
/// sub-agents. `Custom` carries arbitrary caller-defined tags so future
/// instrumentation does not require a schema change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    /// A new user/model turn began.
    TurnStart,
    /// The agent invoked a tool.
    ToolCall,
    /// A tool returned a result.
    ToolResult,
    /// A sub-agent was spawned.
    AgentSpawn,
    /// A sub-agent finished.
    AgentDone,
    /// A recoverable or fatal error was recorded.
    Error,
    /// A deterministic snapshot point (e.g. state committed to disk).
    Checkpoint,
    /// Any other caller-defined event; the inner string is the tag.
    Custom(String),
}

/// A timestamped, sequenced record in the event stream.
///
/// `seq` is assigned by the writer at append time and is strictly increasing
/// within a single log file, so replays can reconstruct ordering even if two
/// events share a wall-clock timestamp. (The writer now lives in
/// [`crate::core::engine::trace::SessionEventSink`]; the envelope schema is
/// still produced by external JSONL logs this module can replay.)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventEnvelope {
    /// Emission time (UTC).
    pub ts: DateTime<Utc>,
    /// Monotonic sequence number assigned by the writer.
    pub seq: u64,
    /// Event category.
    pub kind: EventKind,
    /// Caller-supplied structured payload (tool name, agent id, message…).
    pub payload: Value,
}

impl EventEnvelope {
    /// Build an envelope for `kind` with `payload`, stamping it with the
    /// current UTC time and the given sequence number.
    #[must_use]
    pub fn new(seq: u64, kind: EventKind, payload: Value) -> Self {
        Self {
            ts: Utc::now(),
            seq,
            kind,
            payload,
        }
    }
}

// Deprecated (loop v1 / T3): the `EventLog` writer struct and its
// `EventLogError` type were removed from this module. Trajectory/failure
// events must be emitted through
// `crate::core::engine::trace::SessionEventSink` — the single trajectory
// writer (session.jsonl). The read/replay model below remains for the
// read-only `event_stream` tool.

/// Errors that can occur while reading/replaying a log.
#[derive(Debug, Error)]
pub enum ReplayError {
    /// The log file could not be opened for reading.
    #[error("Failed to open event log for replay {path}: {source}")]
    Open {
        /// Path that failed to open.
        path: PathBuf,
        /// Underlying IO error.
        #[source]
        source: std::io::Error,
    },
}

/// Read a JSON-Lines event log and yield every well-formed [`EventEnvelope`].
///
/// Malformed lines are skipped silently rather than aborting the stream, so a
/// log with a torn final record (crash mid-write) or a stray non-JSON line can
/// still be inspected. The iterator is pure: it performs no I/O writes and
/// leaves `path` untouched.
pub fn replay(path: impl AsRef<Path>) -> Result<impl Iterator<Item = EventEnvelope>, ReplayError> {
    let path = path.as_ref().to_path_buf();
    let file = File::open(&path).map_err(|source| ReplayError::Open {
        path: path.clone(),
        source,
    })?;
    let reader = BufReader::new(file);
    let envelopes = reader
        .lines()
        .map_while(Result::ok)
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str::<EventEnvelope>(&line).ok());
    Ok(envelopes)
}

/// Aggregate counts of events grouped by [`EventKind`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EventCounts {
    /// Per-kind event counts.
    pub by_kind: std::collections::BTreeMap<String, u64>,
    /// Total number of successfully parsed events.
    pub total: u64,
}

impl EventCounts {
    /// Build counts from an iterator of envelopes (typically [`replay`]).
    #[must_use]
    pub fn from_events(events: impl Iterator<Item = EventEnvelope>) -> Self {
        let mut by_kind = std::collections::BTreeMap::new();
        let mut total = 0u64;
        for event in events {
            let key = match &event.kind {
                EventKind::Custom(tag) => format!("custom:{tag}"),
                other => serde_json::to_value(other)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_string))
                    .unwrap_or_else(|| format!("{other:?}")),
            };
            *by_kind.entry(key).or_insert(0) += 1;
            total += 1;
        }
        Self { by_kind, total }
    }

    /// Count of events of the given fixed kind.
    #[must_use]
    pub fn count_of(&self, kind: EventKind) -> u64 {
        let key = match &kind {
            EventKind::Custom(tag) => format!("custom:{tag}"),
            other => serde_json::to_value(other)
                .ok()
                .and_then(|v| v.as_str().map(str::to_string))
                .unwrap_or_else(|| format!("{other:?}")),
        };
        *self.by_kind.get(&key).unwrap_or(&0)
    }
}

/// Pure, side-effect-free reconstruction of a log into queryable form.
///
/// Wraps [`replay`] and provides simple inspectors (count by kind, list tool
/// calls). All methods are pure: none mutate the underlying file.
pub struct EventReplay {
    path: PathBuf,
}

impl EventReplay {
    /// Attach a replay reader to `path`. Reading is lazy and happens on demand.
    #[must_use]
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }

    /// Total parsed event count (malformed lines excluded).
    pub fn count(&self) -> Result<u64, ReplayError> {
        Ok(self.events()?.count() as u64)
    }

    /// Per-kind counts.
    pub fn counts(&self) -> Result<EventCounts, ReplayError> {
        Ok(EventCounts::from_events(self.events()?))
    }

    /// Reconstruct the full ordered event sequence (malformed lines skipped).
    pub fn events(&self) -> Result<impl Iterator<Item = EventEnvelope>, ReplayError> {
        replay(&self.path)
    }

    /// List every `ToolCall` whose payload carries a `tool` field, returning
    /// `(seq, tool_name, payload)` tuples. Pure and panic-free.
    pub fn list_tool_calls(&self) -> Result<Vec<(u64, String, Value)>, ReplayError> {
        let mut out = Vec::new();
        for event in self.events()? {
            if event.kind == EventKind::ToolCall
                && let Some(name) = event.payload.get("tool").and_then(Value::as_str)
            {
                out.push((event.seq, name.to_string(), event.payload));
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn append_then_replay_preserves_count_and_kinds() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("events.jsonl");

        // Writer half was removed (T3: SessionEventSink is the sole writer);
        // seed raw envelope JSONL lines exactly as the legacy writer did.
        let envelopes = [
            EventEnvelope::new(0, EventKind::TurnStart, json!({"turn": 1})),
            EventEnvelope::new(
                1,
                EventKind::ToolCall,
                json!({"tool": "read_file", "path": "x.rs"}),
            ),
            EventEnvelope::new(2, EventKind::Checkpoint, json!({"id": "c1"})),
        ];
        let mut contents = String::new();
        for envelope in &envelopes {
            contents.push_str(&serde_json::to_string(envelope).unwrap());
            contents.push('\n');
        }
        std::fs::write(&path, contents).unwrap();

        // Replay and assert.
        let mut events: Vec<_> = replay(&path).unwrap().collect();
        assert_eq!(events.len(), 3, "three well-formed events expected");
        assert_eq!(events[0].seq, 0);
        assert_eq!(events[0].kind, EventKind::TurnStart);
        assert_eq!(events[1].kind, EventKind::ToolCall);
        assert_eq!(events[1].payload["tool"], "read_file");
        assert_eq!(events[2].kind, EventKind::Checkpoint);

        // Counts helper.
        let counts = EventReplay::new(&path).counts().unwrap();
        assert_eq!(counts.total, 3);
        assert_eq!(counts.count_of(EventKind::TurnStart), 1);
        assert_eq!(counts.count_of(EventKind::ToolCall), 1);
        assert_eq!(counts.count_of(EventKind::Checkpoint), 1);
        assert_eq!(counts.count_of(EventKind::Error), 0);

        // list_tool_calls finds the single tool call.
        let calls = EventReplay::new(&path).list_tool_calls().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].1, "read_file");

        events.clear();
    }

    #[test]
    fn malformed_line_is_skipped_without_panic() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("events.jsonl");

        // Seed a valid line, a garbage line, and a second valid line.
        std::fs::write(
            &path,
            concat!(
                "{\"ts\":\"2026-01-01T00:00:00Z\",\"seq\":0,\"kind\":\"turn_start\",\"payload\":{}}\n",
                "this is not json\n",
                "{\"ts\":\"2026-01-01T00:00:01Z\",\"seq\":1,\"kind\":\"error\",\"payload\":{\"msg\":\"x\"}}\n",
            ),
        )
        .unwrap();

        // Using a hand-rolled valid envelope to satisfy the type checker.
        let mut events: Vec<EventEnvelope> = replay(&path).unwrap().collect();
        assert_eq!(events.len(), 2, "malformed line must be skipped");
        assert_eq!(events[0].kind, EventKind::TurnStart);
        assert_eq!(events[1].kind, EventKind::Error);

        // Replay must not panic and must be re-iterable.
        events = replay(&path).unwrap().collect();
        assert_eq!(events.len(), 2);
    }
}
