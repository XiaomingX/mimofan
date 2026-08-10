use mimofan_memory::*;
use tempfile::TempDir;

fn test_observation() -> Observation {
    Observation {
        id: 1,
        content: "Test observation".to_string(),
        kind: "project".to_string(),
        project: Some("test-project".to_string()),
        files_read: vec!["src/main.rs".to_string()],
        files_modified: vec!["src/lib.rs".to_string()],
        concepts: vec!["bugfix".to_string(), "test".to_string()],
        created_at: chrono::Utc::now().timestamp(),
        access_count: 0,
        last_accessed_at: None,
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

/// #719 — M7 access reinforcement: a successful `search` recall must bump
/// `access_count` and refresh `last_accessed_at` on the recalled observation.
#[test]
fn test_search_records_access() {
    let temp_dir = TempDir::new().expect("create temp dir");
    let store = VectorStore::open(temp_dir.path(), 384).expect("open vector store");

    let obs = test_observation();
    let embedding = vec![0.0; 384];
    let id = store
        .store_observation(&obs, &embedding)
        .expect("store observation");

    // Freshly stored: never accessed yet.
    let before = store.load_observation(id).expect("load").expect("some");
    assert_eq!(before.access_count, 0);
    assert_eq!(before.last_accessed_at, None);

    // A search that recalls the observation increments the counter.
    let results = store
        .search(&embedding, 5, &SearchFilters::default())
        .expect("search");
    assert!(results.iter().any(|m| m.observation.id == id));

    let after = store.load_observation(id).expect("load").expect("some");
    assert_eq!(after.access_count, 1);
    assert!(after.last_accessed_at.is_some());

    // A second recall bumps it to 2 without resetting prior state.
    let _ = store.search(&embedding, 5, &SearchFilters::default());
    let twice = store.load_observation(id).expect("load").expect("some");
    assert_eq!(twice.access_count, 2);
    assert!(twice.last_accessed_at.is_some());
}

/// #719 — explicit `record_access` increments the counter and sets the timestamp.
#[test]
fn test_record_access_direct() {
    let temp_dir = TempDir::new().expect("create temp dir");
    let store = VectorStore::open(temp_dir.path(), 384).expect("open vector store");

    let id = store
        .store_observation(&test_observation(), &vec![0.0; 384])
        .expect("store");

    store.record_access(id).expect("record access");
    let after = store.load_observation(id).expect("load").expect("some");
    assert_eq!(after.access_count, 1);
    assert!(after.last_accessed_at.is_some());
}
