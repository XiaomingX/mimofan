//! Integration tests for `tools::subagent::events`.

use mimofan::tools::subagent::events::{SubAgentEvent, SubAgentEventBus};

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
