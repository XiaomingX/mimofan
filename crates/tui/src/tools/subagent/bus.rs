//! Agent-to-agent peer communication bus.
//!
//! Provides pub/sub message delivery and a shared key-value state space so
//! sub-agents can exchange information without going through the parent.

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::{RwLock, mpsc};

/// A message exchanged between agents through the bus.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BusMessage {
    /// Agent id of the sender.
    pub from: String,
    /// Target agent id. `None` means broadcast to all subscribers of the topic.
    pub to: Option<String>,
    /// Topic channel for pub/sub routing.
    pub topic: String,
    /// Arbitrary JSON payload.
    pub payload: Value,
}

/// Shared agent bus for inter-agent communication.
///
/// All agents spawned by the same [`SubAgentManager`] share a single bus
/// instance (wrapped in `Arc`). Agents subscribe to topics they care about
/// and publish messages that are routed to matching subscribers.
#[derive(Clone)]
pub struct AgentBus {
    inner: Arc<AgentBusInner>,
}

struct AgentBusInner {
    /// Per-topic subscriber channels. Key = topic name, value = list of
    /// unbounded senders (one per subscriber).
    subscribers: RwLock<HashMap<String, Vec<mpsc::UnboundedSender<BusMessage>>>>,
    /// Shared key-value state space. Agents can read/write arbitrary JSON
    /// values keyed by string.
    shared_state: RwLock<HashMap<String, Value>>,
}

/// Handle returned when an agent subscribes to a topic. Dropping it
/// automatically unsubscribes.
pub struct BusSubscription {
    topic: String,
    agent_id: String,
    bus: AgentBus,
}

impl BusSubscription {
    /// Block until the next message arrives on this subscription.
    pub async fn recv(&self, rx: &mut mpsc::UnboundedReceiver<BusMessage>) -> Option<BusMessage> {
        rx.recv().await
    }
}

impl Drop for BusSubscription {
    fn drop(&mut self) {
        // Unsubscribe is best-effort; the bus may already be shutting down.
        let bus = self.bus.clone();
        let topic = self.topic.clone();
        let agent_id = self.agent_id.clone();
        tokio::spawn(async move {
            bus.unsubscribe(&topic, &agent_id).await;
        });
    }
}

impl Default for AgentBus {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentBus {
    /// Create a new empty bus.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(AgentBusInner {
                subscribers: RwLock::new(HashMap::new()),
                shared_state: RwLock::new(HashMap::new()),
            }),
        }
    }

    /// Subscribe to a topic. Returns a receiver that will get messages
    /// published to the topic, and a [`BusSubscription`] guard that
    /// auto-unsubscribes on drop.
    pub async fn subscribe(
        &self,
        topic: &str,
        agent_id: &str,
    ) -> (mpsc::UnboundedReceiver<BusMessage>, BusSubscription) {
        let (tx, rx) = mpsc::unbounded_channel();
        let mut subs = self.inner.subscribers.write().await;
        subs.entry(topic.to_string()).or_default().push(tx);

        let sub = BusSubscription {
            topic: topic.to_string(),
            agent_id: agent_id.to_string(),
            bus: self.clone(),
        };
        (rx, sub)
    }

    /// Remove all subscriptions belonging to `agent_id` on the given topic.
    async fn unsubscribe(&self, topic: &str, agent_id: &str) {
        // We cannot identify which sender belongs to `agent_id` directly
        // because `mpsc::UnboundedSender` does not carry identity. Instead,
        // we remove dead (disconnected) senders as a cleanup pass. This is
        // sufficient because BusSubscription::drop fires on agent teardown.
        let mut subs = self.inner.subscribers.write().await;
        if let Some(senders) = subs.get_mut(topic) {
            // UnboundedSender has no capacity() — unlimited buffer.
            // Only check whether the receiver is still alive.
            senders.retain(|s| !s.is_closed());
            if senders.is_empty() {
                subs.remove(topic);
            }
        }
        let _ = agent_id; // reserved for future per-agent tracking
    }

    /// Publish a message. If `message.to` is `Some(id)`, the message is
    /// delivered only to subscribers whose topic matches; broadcast messages
    /// (`to = None`) are delivered to all subscribers of the topic.
    pub async fn publish(&self, message: BusMessage) {
        let subs = self.inner.subscribers.read().await;
        if let Some(senders) = subs.get(&message.topic) {
            for tx in senders {
                // Best-effort delivery; ignore closed channels.
                let _ = tx.send(message.clone());
            }
        }
    }

    /// Set a value in the shared state space.
    pub async fn state_set(&self, key: &str, value: Value) {
        let mut state = self.inner.shared_state.write().await;
        state.insert(key.to_string(), value);
    }

    /// Get a value from the shared state space.
    pub async fn state_get(&self, key: &str) -> Option<Value> {
        let state = self.inner.shared_state.read().await;
        state.get(key).cloned()
    }

    /// Remove a value from the shared state space. Returns the old value if
    /// it existed.
    pub async fn state_remove(&self, key: &str) -> Option<Value> {
        let mut state = self.inner.shared_state.write().await;
        state.remove(key)
    }

    /// List all keys in the shared state space.
    pub async fn state_keys(&self) -> Vec<String> {
        let state = self.inner.shared_state.read().await;
        state.keys().cloned().collect()
    }
}
