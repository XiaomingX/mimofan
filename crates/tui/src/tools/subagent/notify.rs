//! Notification primitive for sub-agents (#867).
//!
//! Agents can push an external notification (e.g. "I'm blocked and need
//! human input", or "I'm done") through a pluggable `NotificationSink`.
//! The default sink writes to stderr so it is safe inside the alt-screen
//! TUI (raw stderr routed through the file-backed subscriber leaks into the
//! buffer — see the `#freeze` note in `manager.rs`; callers should route
//! display through `tracing`). The sink is intentionally dependency-free:
//! no network, no desktop-notification crates.

use std::sync::{Arc, Mutex};

/// A pluggable destination for agent notifications (#867).
///
/// Implement this trait to deliver notifications anywhere (stderr, a
/// channel, a file, a UI queue). The default implementation is
/// [`StderrSink`].
pub trait NotificationSink: Send + Sync {
    /// Push a notification message for the given agent.
    fn notify(&self, agent_id: &str, message: &str);
}

/// Default sink: writes to stderr in a single line.
///
/// Safe to construct with no external dependencies. Ordering across agents
/// is preserved per-sink because the underlying `eprintln!` is internally
/// synchronized.
#[derive(Debug, Default, Clone)]
pub struct StderrSink;

impl NotificationSink for StderrSink {
    fn notify(&self, agent_id: &str, message: &str) {
        eprintln!("[notify] agent {agent_id}: {message}");
    }
}

/// Alternative default sink: writes to stdout.
#[derive(Debug, Default, Clone)]
pub struct StdoutSink;

impl NotificationSink for StdoutSink {
    #[allow(clippy::print_stdout)]
    fn notify(&self, agent_id: &str, message: &str) {
        println!("[notify] agent {agent_id}: {message}");
    }
}

/// In-memory sink used by tests to assert a notification was emitted without
/// touching any real I/O.
#[derive(Debug, Default, Clone)]
pub struct CapturingSink {
    messages: Arc<Mutex<Vec<(String, String)>>>,
}

impl CapturingSink {
    /// Return a clone of all captured `(agent_id, message)` pairs so far.
    #[must_use]
    pub fn captured(&self) -> Vec<(String, String)> {
        self.messages
            .lock()
            .expect("capturing sink poisoned")
            .clone()
    }

    /// Whether any notification for `agent_id` has been captured.
    #[must_use]
    pub fn was_notified(&self, agent_id: &str) -> bool {
        self.messages
            .lock()
            .expect("capturing sink poisoned")
            .iter()
            .any(|(id, _)| id == agent_id)
    }
}

impl NotificationSink for CapturingSink {
    fn notify(&self, agent_id: &str, message: &str) {
        self.messages
            .lock()
            .expect("capturing sink poisoned")
            .push((agent_id.to_string(), message.to_string()));
    }
}

/// Dispatches agent notifications to a pluggable [`NotificationSink`] (#867).
///
/// Cheaply cloneable (the sink is behind `Arc`); the default sink writes to
/// stderr. Swap the sink via [`Notifier::with_sink`] for tests or other
/// delivery backends.
#[derive(Clone)]
pub struct Notifier {
    sink: Arc<dyn NotificationSink>,
}

impl Default for Notifier {
    fn default() -> Self {
        Self {
            sink: Arc::new(StderrSink),
        }
    }
}

impl Notifier {
    /// Create a notifier with the default stderr sink.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a notifier that delivers through a custom sink.
    #[must_use]
    pub fn with_sink(sink: Arc<dyn NotificationSink>) -> Self {
        Self { sink }
    }

    /// Push a notification from `agent_id`.
    pub fn notify(&self, agent_id: &str, message: &str) {
        self.sink.notify(agent_id, message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notifier_invokes_sink() {
        let sink = Arc::new(CapturingSink::default());
        let notifier = Notifier::with_sink(Arc::clone(&sink) as Arc<dyn NotificationSink>);
        notifier.notify("agent-7", "blocked: needs approval");
        assert!(sink.was_notified("agent-7"));
        let captured = sink.captured();
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].0, "agent-7");
        assert_eq!(captured[0].1, "blocked: needs approval");
    }

    #[test]
    fn stderr_sink_is_default() {
        let notifier = Notifier::new();
        // Should not panic; writes to stderr.
        notifier.notify("a", "done");
    }
}
