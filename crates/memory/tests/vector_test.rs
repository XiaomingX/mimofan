use mimofan_memory::*;
use tempfile::TempDir;

fn test_observation() -> Observation {
    Observation {
        id: 1,
        content: "Test observation".to_string(),
        kind: ObservationKind::Bugfix,
        project: Some("test-project".to_string()),
        files_read: vec!["src/main.rs".to_string()],
        files_modified: vec!["src/lib.rs".to_string()],
        concepts: vec!["bugfix".to_string(), "test".to_string()],
        created_at: chrono::Utc::now().timestamp(),
    }
}

#[test]
fn test_vector_store_creation() {
    let temp_dir = TempDir::new().expect("create temp dir");
    let store = VectorStore::open(temp_dir.path(), 384);
    assert!(store.is_ok());
}

#[test]
fn test_store_and_load_observation() {
    let temp_dir = TempDir::new().expect("create temp dir");
    let store = VectorStore::open(temp_dir.path(), 384).expect("open vector store");

    let observation = test_observation();
    let embedding = vec![0.0; 384];

    let id = store
        .store_observation(&observation, &embedding)
        .expect("store observation");
    assert!(id > 0);

    let loaded = store.load_observation(id).expect("load observation");
    assert!(loaded.is_some());
    let loaded = loaded.expect("unwrap loaded observation");
    assert_eq!(loaded.content, observation.content);
    assert_eq!(loaded.kind, observation.kind);
}

#[test]
fn test_search() {
    let temp_dir = TempDir::new().expect("create temp dir");
    let store = VectorStore::open(temp_dir.path(), 384).expect("open vector store");

    // Store some observations
    for i in 0..5 {
        let mut obs = test_observation();
        obs.id = i;
        obs.content = format!("Observation {}", i);
        let embedding = vec![i as f32; 384];
        store
            .store_observation(&obs, &embedding)
            .expect("store observation");
    }

    // Search
    let query = vec![0.0; 384];
    let results = store
        .search(&query, 3, &SearchFilters::default())
        .expect("search vector store");
    assert!(!results.is_empty());
}
