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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn test_publish_subscribe() {
        let bus = AgentBus::new();
        let (mut rx, _sub) = bus.subscribe("test_topic", "agent_a").await;

        let msg = BusMessage {
            from: "agent_b".into(),
            to: None,
            topic: "test_topic".into(),
            payload: json!({"hello": "world"}),
        };
        bus.publish(msg.clone()).await;

        let received = rx.recv().await.expect("should receive message");
        assert_eq!(received.from, "agent_b");
        assert_eq!(received.topic, "test_topic");
        assert_eq!(received.payload, json!({"hello": "world"}));
    }

    #[tokio::test]
    async fn test_shared_state() {
        let bus = AgentBus::new();
        bus.state_set("key1", json!("value1")).await;
        assert_eq!(bus.state_get("key1").await, Some(json!("value1")));

        let old = bus.state_remove("key1").await;
        assert_eq!(old, Some(json!("value1")));
        assert!(bus.state_get("key1").await.is_none());
    }

    #[tokio::test]
    async fn test_multiple_subscribers() {
        let bus = AgentBus::new();
        let (mut rx1, _sub1) = bus.subscribe("chat", "agent_a").await;
        let (mut rx2, _sub2) = bus.subscribe("chat", "agent_b").await;

        let msg = BusMessage {
            from: "agent_c".into(),
            to: None,
            topic: "chat".into(),
            payload: json!("hi"),
        };
        bus.publish(msg).await;

        assert!(rx1.recv().await.is_some());
        assert!(rx2.recv().await.is_some());
    }

    #[tokio::test]
    async fn test_unrelated_topic_not_delivered() {
        let bus = AgentBus::new();
        let (mut rx, _sub) = bus.subscribe("topic_a", "agent_a").await;

        let msg = BusMessage {
            from: "agent_b".into(),
            to: None,
            topic: "topic_b".into(),
            payload: json!("nope"),
        };
        bus.publish(msg).await;

        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn test_state_keys() {
        let bus = AgentBus::new();
        bus.state_set("a", json!(1)).await;
        bus.state_set("b", json!(2)).await;
        let mut keys = bus.state_keys().await;
        keys.sort();
        assert_eq!(keys, vec!["a", "b"]);
    }
}
