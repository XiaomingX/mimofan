use mimofan_memory::*;

#[test]
fn test_injection_config_default() {
    let config = InjectionConfig::default();
    assert_eq!(config.max_observations, 10);
    assert_eq!(config.max_tokens, 4000);
    assert_eq!(config.context_depth, 3);
}

#[test]
fn test_memory_injection_serialization() {
    let injection = MemoryInjection {
        summary: "Test summary".to_string(),
        key_decisions: vec!["Decision 1".to_string()],
        recent_changes: vec!["Change 1".to_string()],
        files_modified: vec!["src/main.rs".to_string()],
        estimated_tokens: 100,
    };

    let json = serde_json::to_string(&injection).expect("serialize injection to json");
    let deserialized: MemoryInjection =
        serde_json::from_str(&json).expect("parse injection json");

    assert_eq!(deserialized.summary, injection.summary);
    assert_eq!(deserialized.key_decisions, injection.key_decisions);
}
