use mimofan_memory::optimization::SearchCache;
use mimofan_memory::vector::{MemoryOrigin, Observation, VectorMatch};

fn fake_match(content: &str, score: f32) -> VectorMatch {
    VectorMatch {
        observation: Observation {
            id: 0,
            content: content.to_string(),
            kind: "project".to_string(),
            project: Some("demo".to_string()),
            files_read: Vec::new(),
            files_modified: Vec::new(),
            concepts: Vec::new(),
            created_at: 0,
            access_count: 0,
            last_accessed_at: None,
            expires_at: None,
            session_id: "test".to_string(),
            origin: MemoryOrigin::User,
        },
        score,
    }
}

#[test]
fn search_cache_miss_then_hit() {
    let cache = SearchCache::new(8);

    // Miss on empty cache
    assert!(cache.get("q:1536:5:demo").is_none());
    assert!(cache.is_empty());

    // Insert a result, then hit
    let results = vec![fake_match("renamed module", 0.91)];
    cache.insert("q:1536:5:demo".to_string(), results);
    assert_eq!(cache.len(), 1);

    let hit = cache.get("q:1536:5:demo").expect("should be a cache hit");
    assert_eq!(hit.len(), 1);
    assert_eq!(hit[0].observation.content, "renamed module");
    assert!((hit[0].score - 0.91).abs() < f32::EPSILON);
}

#[test]
fn search_cache_lru_eviction() {
    let cache = SearchCache::new(2);
    cache.insert("a".to_string(), vec![fake_match("a", 0.1)]);
    cache.insert("b".to_string(), vec![fake_match("b", 0.2)]);
    assert_eq!(cache.len(), 2);

    // Inserting a third entry evicts the least-recently-used ("a").
    cache.insert("c".to_string(), vec![fake_match("c", 0.3)]);
    assert_eq!(cache.len(), 2);
    assert!(cache.get("a").is_none(), "LRU entry 'a' should be evicted");
    assert!(cache.get("b").is_some());
    assert!(cache.get("c").is_some());
}

#[test]
fn search_cache_clear() {
    let cache = SearchCache::new(4);
    cache.insert("k".to_string(), vec![fake_match("x", 0.5)]);
    assert!(!cache.is_empty());
    cache.clear();
    assert!(cache.is_empty());
    assert!(cache.get("k").is_none());
}
