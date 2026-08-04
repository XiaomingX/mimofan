//! Integration tests for `tools::subagent::naming`.

use std::collections::HashSet;

use mimofan::tools::subagent::{assign_unique_whale_name, whale_name_for_id};

#[test]
fn test_whale_name_deterministic() {
    let name1 = whale_name_for_id("agent-123");
    let name2 = whale_name_for_id("agent-123");
    assert_eq!(name1, name2);
}

#[test]
fn test_assign_unique_whale_name_no_collision() {
    let mut active = HashSet::new();
    let name = assign_unique_whale_name("agent-123", &active);
    assert!(!name.is_empty());
    active.insert(name);
}

#[test]
fn test_assign_unique_whale_name_with_collision() {
    let mut active = HashSet::new();
    let name1 = assign_unique_whale_name("agent-123", &active);
    active.insert(name1.clone());
    let name2 = assign_unique_whale_name("agent-456", &active);
    assert_ne!(name1, name2);
}
