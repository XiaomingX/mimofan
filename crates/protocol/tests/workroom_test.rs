//! Externalized integration tests for `crates/protocol/src/workroom.rs`.
//!
//! Originally an inline `#[cfg(test)] mod tests` block — relocated here as a
//! separate integration-test crate without any change to test logic.

use mimofan_protocol::workroom::*;

#[test]
fn workroom_id_new_is_stable() {
    let id = WorkroomId::new();
    assert!(id.0.starts_with("wr_"));
    assert_eq!(id.0.len(), 35); // "wr_" + 32 hex chars
}

#[test]
fn workroom_link_parse_workroom_only() {
    let link = WorkroomLink::parse("mimofan://workroom/wr_abc123def456").expect("parse workroom-only link");
    assert_eq!(link.workroom_id.0, "wr_abc123def456");
    assert!(link.thread_id.is_none());
    assert!(link.event_id.is_none());
}

#[test]
fn workroom_link_parse_with_thread() {
    let link = WorkroomLink::parse("mimofan://workroom/wr_abc/thread/thr_xyz").expect("parse link with thread");
    assert_eq!(link.workroom_id.0, "wr_abc");
    assert_eq!(link.thread_id.as_deref(), Some("thr_xyz"));
    assert!(link.event_id.is_none());
}

#[test]
fn workroom_link_parse_with_event() {
    let link = WorkroomLink::parse("mimofan://workroom/wr_abc/event/evt_789").expect("parse link with event");
    assert_eq!(link.workroom_id.0, "wr_abc");
    assert_eq!(link.event_id.as_deref(), Some("evt_789"));
    assert!(link.thread_id.is_none());
}

#[test]
fn workroom_link_roundtrip() {
    let original = "mimofan://workroom/wr_abc/thread/thr_x/event/evt_y";
    let parsed = WorkroomLink::parse(original).expect("roundtrip parse workroom link");
    assert_eq!(parsed.to_url(), original);
}

#[test]
fn workroom_link_reject_bad_prefix() {
    assert!(WorkroomLink::parse("http://workroom/wr_abc").is_none());
    assert!(WorkroomLink::parse("mimofan://not-workroom/wr_abc").is_none());
}

#[test]
fn workroom_link_rejects_malformed_paths() {
    assert!(WorkroomLink::parse("mimofan://workroom/").is_none());
    assert!(WorkroomLink::parse("mimofan://workroom/abc").is_none());
    assert!(WorkroomLink::parse("mimofan://workroom/wr_").is_none());
    assert!(WorkroomLink::parse("mimofan://workroom/wr_abc/thread").is_none());
    assert!(WorkroomLink::parse("mimofan://workroom/wr_abc/thread/").is_none());
    assert!(WorkroomLink::parse("mimofan://workroom/wr_abc/unknown/x").is_none());
    assert!(WorkroomLink::parse("mimofan://workroom/wr_abc/event/evt/x").is_none());
}

#[test]
fn external_thread_ref_serde_roundtrip() {
    let issue = ExternalThreadRef::GitHubIssue {
        owner: "Hmbown".into(),
        repo: "mimofan".into(),
        number: 3209,
    };
    let json = serde_json::to_string(&issue).expect("serialize external thread ref");
    let back: ExternalThreadRef = serde_json::from_str(&json).expect("deserialize external thread ref");
    assert!(matches!(back, ExternalThreadRef::GitHubIssue { .. }));
}

#[test]
fn agent_attribution_serde_roundtrip() {
    let attr = AgentAttribution {
        provider: "deepseek".into(),
        model: "deepseek-v4-pro".into(),
        agent_id: "sub_agent_1".into(),
    };
    let json = serde_json::to_string(&attr).expect("serialize agent attribution");
    let back: AgentAttribution = serde_json::from_str(&json).expect("deserialize agent attribution");
    assert_eq!(back.provider, "deepseek");
    assert_eq!(back.model, "deepseek-v4-pro");
}
