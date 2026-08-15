//! #850 — Structured event stream (jsonl) + replay.
//!
//! An append-only, line-delimited JSON log of mimofan's execution. Each entry
//! is an [`EventEnvelope`] carrying a monotonic sequence number, an emission
//! timestamp, a [`EventKind`] tag, and an arbitrary JSON `payload`. The log is
//! meant to be durable and machine-inspectable so a session can be replayed,
//! audited, or turned into metrics without re-running the agent.
//!
//! This module owns the data model and I/O only. Tool registration is deferred
//! (see `EventStreamTool` which is implemented but not wired into the registry
//! yet), so nothing here reaches into `mod.rs`, `registry.rs`, or `engine.rs`.

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
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
/// `seq` is assigned by [`EventLog`] at append time and is strictly increasing
/// within a single log file, so replays can reconstruct ordering even if two
/// events share a wall-clock timestamp.
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

/// Errors that can occur while opening, appending to, or reading an event log.
#[derive(Debug, Error)]
pub enum EventLogError {
    /// The log file could not be opened for writing.
    #[error("Failed to open event log {path}: {source}")]
    Open {
        /// Path that failed to open.
        path: PathBuf,
        /// Underlying IO error.
        #[source]
        source: std::io::Error,
    },
    /// An event could not be serialized to JSON.
    #[error("Failed to serialize event: {0}")]
    Serialize(#[from] serde_json::Error),
    /// An event could not be flushed to disk.
    #[error("Failed to write event log {path}: {source}")]
    Write {
        /// Path that failed to write.
        path: PathBuf,
        /// Underlying IO error.
        #[source]
        source: std::io::Error,
    },
}

/// An append-only JSON-Lines event log.
///
/// `open` creates (or appends to) a file at `path`. Each [`EventLog::append`]
/// writes exactly one JSON object followed by `\n` and flushes, so a crash
/// between events never produces a torn record — the worst case is a missing
/// final line, which [`replay`] already tolerates.
pub struct EventLog {
    path: PathBuf,
    file: File,
    next_seq: u64,
}

impl EventLog {
    /// Open (creating if absent) the event log at `path`.
    ///
    /// The sequence counter starts at 0; callers that need continuity across
    /// reopen should `replay` first and set `next_seq` from the last seen `seq`.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, EventLogError> {
        let path = path.as_ref().to_path_buf();
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|source| EventLogError::Open {
                path: path.clone(),
                source,
            })?;
        Ok(Self {
            path,
            file,
            next_seq: 0,
        })
    }

    /// Path this log writes to.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Append one event of `kind` with `payload`, returning the assigned
    /// sequence number. The write is flushed before returning.
    pub fn append(
        &mut self,
        kind: EventKind,
        payload: Value,
    ) -> Result<u64, EventLogError> {
        let seq = self.next_seq;
        let envelope = EventEnvelope::new(seq, kind, payload);
        let mut line = serde_json::to_string(&envelope)?;
        line.push('\n');
        self.file
            .write_all(line.as_bytes())
            .map_err(|source| EventLogError::Write {
                path: self.path.clone(),
                source,
            })?;
        self.file
            .flush()
            .map_err(|source| EventLogError::Write {
                path: self.path.clone(),
                source,
            })?;
        self.next_seq = self.next_seq.saturating_add(1);
        Ok(seq)
    }
}

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
        .filter_map(Result::ok)
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
                other => serde_json::to_value(&other)
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
            other => serde_json::to_value(&other)
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
    pub fn list_tool_calls(
        &self,
    ) -> Result<Vec<(u64, String, Value)>, ReplayError> {
        let mut out = Vec::new();
        for event in self.events()? {
            if event.kind == EventKind::ToolCall {
                if let Some(name) = event.payload.get("tool").and_then(Value::as_str) {
                    out.push((event.seq, name.to_string(), event.payload));
                }
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

        // Write 3 events: TurnStart, ToolCall, Checkpoint.
        let mut log = EventLog::open(&path).unwrap();
        let s0 = log.append(EventKind::TurnStart, json!({"turn": 1})).unwrap();
        let s1 = log
            .append(EventKind::ToolCall, json!({"tool": "read_file", "path": "x.rs"}))
            .unwrap();
        let s2 = log.append(EventKind::Checkpoint, json!({"id": "c1"})).unwrap();
        assert_eq!((s0, s1, s2), (0, 1, 2));

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

        // Drop the log handle first so the temp file's sequence is observable.
        drop(log);
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
