//! Mailbox abstraction for sub-agent runtime coordination.
//!
//! Monotonic sequence numbers give every consumer a consistent ordering even
//! when multiple subscribers (e.g. UI card + parent agent) drain
//! independently; close-as-cancel lets a single signal both stop new mail and
//! propagate cancellation through nested children.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;

use crate::models::Usage;

use super::SubAgentType;

/// Stable, structured progress envelope shared across the sub-agent surface.
///
/// Tracks the lifecycle of a single agent (identified by `agent_id`) end to
/// end: spawn, per-step progress, tool execution, completion / failure /
/// cancellation, and parent → child topology so consumers can render trees.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MailboxMessage {
    /// Agent has been started (background task is running).
    Started {
        agent_id: String,
        agent_type: String,
    },
    /// Free-form human-readable progress (mirrors `Event::AgentProgress`).
    Progress { agent_id: String, status: String },
    /// A tool call inside the agent has started.
    ToolCallStarted {
        agent_id: String,
        tool_name: String,
        step: u32,
    },
    /// A tool call inside the agent has finished.
    ToolCallCompleted {
        agent_id: String,
        tool_name: String,
        step: u32,
        ok: bool,
    },
    /// A child agent was spawned by this agent.
    ChildSpawned { parent_id: String, child_id: String },
    /// Agent completed successfully (carries the summary line shown in the
    /// transcript; full result is still available through the transcript handle).
    Completed { agent_id: String, summary: String },
    /// Agent failed with the carried error message.
    Failed { agent_id: String, error: String },
    /// Agent was interrupted (e.g. API timeout) with a continuable
    /// checkpoint; the worker is parked waiting for continuation input.
    Interrupted { agent_id: String, reason: String },
    /// Cancellation propagated to this agent.
    Cancelled { agent_id: String },
    /// Incremental token usage from a sub-agent's API call.
    /// Published after each turn so the parent's cost counter updates live.
    TokenUsage {
        agent_id: String,
        /// Model that produced this usage, used for pricing.
        model: String,
        /// Provider usage payload, including cache-hit/cache-miss fields.
        usage: Usage,
    },
}

impl MailboxMessage {
    /// `agent_id` of the message subject (for `ChildSpawned` this is the
    /// child, since that's the new lifecycle being announced).
    #[must_use]
    pub fn agent_id(&self) -> &str {
        match self {
            Self::Started { agent_id, .. }
            | Self::Progress { agent_id, .. }
            | Self::ToolCallStarted { agent_id, .. }
            | Self::ToolCallCompleted { agent_id, .. }
            | Self::Completed { agent_id, .. }
            | Self::Failed { agent_id, .. }
            | Self::Interrupted { agent_id, .. }
            | Self::Cancelled { agent_id }
            | Self::TokenUsage { agent_id, .. } => agent_id,
            Self::ChildSpawned { child_id, .. } => child_id,
        }
    }

    pub(crate) fn started(agent_id: impl Into<String>, agent_type: SubAgentType) -> Self {
        Self::Started {
            agent_id: agent_id.into(),
            agent_type: agent_type.as_str().to_string(),
        }
    }

    pub(crate) fn progress(agent_id: impl Into<String>, status: impl Into<String>) -> Self {
        Self::Progress {
            agent_id: agent_id.into(),
            status: status.into(),
        }
    }

    pub(crate) fn token_usage(
        agent_id: impl Into<String>,
        model: impl Into<String>,
        usage: Usage,
    ) -> Self {
        Self::TokenUsage {
            agent_id: agent_id.into(),
            model: model.into(),
            usage,
        }
    }
}

/// One delivery: a sequence number plus the message. The sequence is
/// monotonic across the entire mailbox (not per-agent) so a single ordering
/// is well-defined even when multiple sub-agents share one mailbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailboxEnvelope {
    pub seq: u64,
    pub message: MailboxMessage,
}

/// Sender side of the mailbox.
///
/// Cheaply cloneable (everything inside is `Arc`/atomic). Cloning a
/// `Mailbox` shares the same delivery channel, sequence counter, watch
/// notifier, and close/cancel state — so a child runtime that clones its
/// parent's `Mailbox` participates in the same stream.
#[derive(Clone)]
pub struct Mailbox {
    inner: Arc<MailboxInner>,
}

struct MailboxInner {
    tx: mpsc::UnboundedSender<MailboxEnvelope>,
    next_seq: AtomicU64,
    seq_tx: watch::Sender<u64>,
    closed: AtomicBool,
    #[cfg(test)]
    cancel_token: CancellationToken,
}

/// Receiver side of the mailbox. Not `Clone` — only the original creator
/// can drain. Use `Mailbox::subscribe()` for fanout (UI cards + parent both
/// observing the same stream).
pub struct MailboxReceiver {
    rx: mpsc::UnboundedReceiver<MailboxEnvelope>,
    pending: VecDeque<MailboxEnvelope>,
}

impl Mailbox {
    /// Create a new mailbox bound to the given cancellation token. Closing
    /// the mailbox (or dropping the last sender) cancels this token. Runtimes
    /// that derive from the same token observe that cancellation; detached
    /// background `agent` sessions use their own runtime token.
    #[must_use]
    pub fn new(cancel_token: CancellationToken) -> (Self, MailboxReceiver) {
        #[cfg(not(test))]
        let _ = cancel_token;
        let (tx, rx) = mpsc::unbounded_channel();
        let (seq_tx, _) = watch::channel(0);
        let inner = MailboxInner {
            tx,
            next_seq: AtomicU64::new(0),
            seq_tx,
            closed: AtomicBool::new(false),
            #[cfg(test)]
            cancel_token,
        };
        (
            Self {
                inner: Arc::new(inner),
            },
            MailboxReceiver {
                rx,
                pending: VecDeque::new(),
            },
        )
    }

    /// Subscribe to seq-bump notifications. Each `recv()` returns when the
    /// sequence counter advances, signaling new mail without copying it —
    /// the consumer then calls `drain` (or `recv_one` on its own receiver).
    /// Multiple subscribers may exist; this is the fanout primitive.
    #[cfg(test)]
    #[must_use]
    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.inner.seq_tx.subscribe()
    }

    /// Send a message; returns `Some(seq)` on success, `None` if the
    /// mailbox is already closed (callers should treat this as "the
    /// receiver is gone, stop publishing").
    pub fn send(&self, message: MailboxMessage) -> Option<u64> {
        if self.inner.closed.load(Ordering::Acquire) {
            return None;
        }
        let seq = self.inner.next_seq.fetch_add(1, Ordering::Relaxed) + 1;
        let envelope = MailboxEnvelope { seq, message };
        if self.inner.tx.send(envelope).is_err() {
            return None;
        }
        let _ = self.inner.seq_tx.send_replace(seq);
        Some(seq)
    }

    /// Whether the mailbox has been closed.
    #[cfg(test)]
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.inner.closed.load(Ordering::Acquire)
    }
}

impl MailboxReceiver {
    /// Await the next envelope, with backpressure-aware blocking. Returns
    /// `None` when every sender has been dropped and the buffer is drained.
    pub async fn recv(&mut self) -> Option<MailboxEnvelope> {
        if let Some(env) = self.pending.pop_front() {
            return Some(env);
        }
        self.rx.recv().await
    }
}
