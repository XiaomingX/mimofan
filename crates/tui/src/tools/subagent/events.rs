//! Sub-agent event handling.
//!
//! Event types and processing for sub-agent lifecycle management.

use serde::{Deserialize, Serialize};

use super::types::SubAgentStatus;

/// Event types for sub-agent lifecycle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SubAgentEvent {
    /// Agent spawned.
    Spawned {
        agent_id: String,
        agent_type: String,
        model: String,
    },
    /// Agent status changed.
    StatusChanged {
        agent_id: String,
        old_status: SubAgentStatus,
        new_status: SubAgentStatus,
    },
    /// Agent produced output.
    Output { agent_id: String, content: String },
    /// Agent encountered an error.
    Error { agent_id: String, error: String },
    /// Agent completed.
    Completed {
        agent_id: String,
        result: Option<String>,
        duration_ms: u64,
    },
    /// Agent cancelled.
    Cancelled { agent_id: String, reason: String },
    /// Agent needs input from parent.
    NeedsInput { agent_id: String, question: String },
    /// Agent step completed.
    StepCompleted {
        agent_id: String,
        step: u32,
        total_steps: u32,
    },
}

impl SubAgentEvent {
    /// Get the agent ID for this event.
    #[must_use]
    pub fn agent_id(&self) -> &str {
        match self {
            Self::Spawned { agent_id, .. }
            | Self::StatusChanged { agent_id, .. }
            | Self::Output { agent_id, .. }
            | Self::Error { agent_id, .. }
            | Self::Completed { agent_id, .. }
            | Self::Cancelled { agent_id, .. }
            | Self::NeedsInput { agent_id, .. }
            | Self::StepCompleted { agent_id, .. } => agent_id,
        }
    }

    /// Get the event type as a string.
    #[must_use]
    pub fn event_type(&self) -> &'static str {
        match self {
            Self::Spawned { .. } => "spawned",
            Self::StatusChanged { .. } => "status_changed",
            Self::Output { .. } => "output",
            Self::Error { .. } => "error",
            Self::Completed { .. } => "completed",
            Self::Cancelled { .. } => "cancelled",
            Self::NeedsInput { .. } => "needs_input",
            Self::StepCompleted { .. } => "step_completed",
        }
    }

    /// Get the timestamp for this event.
    #[must_use]
    pub fn timestamp(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    }
}

/// Event bus for sub-agent events.
#[derive(Debug, Default)]
pub struct SubAgentEventBus {
    events: Vec<SubAgentEvent>,
}

impl SubAgentEventBus {
    /// Create a new event bus.
    #[must_use]
    pub fn new() -> Self {
        Self { events: Vec::new() }
    }

    /// Push an event to the bus.
    pub fn push(&mut self, event: SubAgentEvent) {
        self.events.push(event);
    }

    /// Get all events.
    #[must_use]
    pub fn events(&self) -> &[SubAgentEvent] {
        &self.events
    }

    /// Get events for a specific agent.
    #[must_use]
    pub fn events_for_agent(&self, agent_id: &str) -> Vec<&SubAgentEvent> {
        self.events
            .iter()
            .filter(|e| e.agent_id() == agent_id)
            .collect()
    }

    /// Clear all events.
    pub fn clear(&mut self) {
        self.events.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sub_agent_event_agent_id() {
        let event = SubAgentEvent::Spawned {
            agent_id: "agent-123".to_string(),
            agent_type: "general".to_string(),
            model: "gpt-4".to_string(),
        };
        assert_eq!(event.agent_id(), "agent-123");
    }

    #[test]
    fn test_sub_agent_event_type() {
        let event = SubAgentEvent::Spawned {
            agent_id: "agent-123".to_string(),
            agent_type: "general".to_string(),
            model: "gpt-4".to_string(),
        };
        assert_eq!(event.event_type(), "spawned");
    }

    #[test]
    fn test_sub_agent_event_bus() {
        let mut bus = SubAgentEventBus::new();
        let event = SubAgentEvent::Spawned {
            agent_id: "agent-123".to_string(),
            agent_type: "general".to_string(),
            model: "gpt-4".to_string(),
        };
        bus.push(event);
        assert_eq!(bus.events().len(), 1);
        assert_eq!(bus.events_for_agent("agent-123").len(), 1);
        assert_eq!(bus.events_for_agent("agent-456").len(), 0);
    }
}
