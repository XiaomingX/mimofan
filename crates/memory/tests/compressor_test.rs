use chrono::Utc;
use mimofan_memory::*;

fn test_observations() -> Vec<Observation> {
    let now = Utc::now().timestamp();
    vec![
        Observation {
            id: 1,
            content: "Fixed authentication bug".to_string(),
            kind: ObservationKind::Bugfix,
            project: Some("test-project".to_string()),
            files_read: vec!["src/auth.rs".to_string()],
            files_modified: vec!["src/auth.rs".to_string()],
            concepts: vec!["authentication".to_string()],
            created_at: now - 3600,
        },
        Observation {
            id: 2,
            content: "Added login feature".to_string(),
            kind: ObservationKind::Feature,
            project: Some("test-project".to_string()),
            files_read: vec!["src/login.rs".to_string()],
            files_modified: vec!["src/login.rs".to_string()],
            concepts: vec!["authentication".to_string()],
            created_at: now - 1800,
        },
        Observation {
            id: 3,
            content: "Decided to use JWT".to_string(),
            kind: ObservationKind::Decision,
            project: Some("test-project".to_string()),
            files_read: vec![],
            files_modified: vec![],
            concepts: vec!["authentication".to_string(), "jwt".to_string()],
            created_at: now,
        },
    ]
}

#[test]
fn test_compressor_creation() {
    let compressor = ObservationCompressor::new();
    assert_eq!(compressor.min_observations, 100);
    assert_eq!(compressor.max_age_seconds, 7 * 24 * 60 * 60);
}

#[test]
fn test_analyze_observations_keep_all() {
    let compressor = ObservationCompressor::new();
    let observations = test_observations();

    let strategies = compressor.analyze_observations(&observations);
    assert_eq!(strategies.len(), 3);

    for strategy in &strategies {
        assert!(matches!(strategy, CompressionStrategy::Keep));
    }
}

#[test]
fn test_summarize_session() {
    let compressor = ObservationCompressor::new();
    let observations = test_observations();

    let summary = compressor
        .summarize_session("test-session", &observations)
        .expect("summarize test session");

    assert_eq!(summary.session_id, "test-session");
    assert_eq!(summary.total_observations, 3);
    assert_eq!(summary.key_decisions.len(), 1);
    assert!(!summary.summary.is_empty());
}
