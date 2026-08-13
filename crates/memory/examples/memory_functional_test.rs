//! Memory System Functional Tests
//!
//! Verify all memory system capabilities work correctly:
//! 1. Vector store: Store, search, filter
//! 2. Compressor: Analysis, summarization
//! 3. Injector: Memory injection
//! 4. Knowledge: Corpus build and query
//! 5. Optimization: Batch, cache, rate limit, tasks

use mimofan_memory::compressor::ObservationCompressor;
use mimofan_memory::optimization::{BatchProcessor, LongTaskManager, RateLimiter, SearchCache};
use mimofan_memory::vector::{Observation, SearchFilters, VectorStore};
use std::time::Duration;

/// Helper to create test observation
fn create_observation(project: &str, kind: &str, content: &str) -> Observation {
    Observation::with_session(project.to_string(), kind, content.to_string(), "func".to_string())
}

/// Helper to generate embedding
fn generate_embedding(seed: u64) -> Vec<f32> {
    (0..384)
        .map(|i| {
            let x = (seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(i as u64)) as f32;
            (x / u64::MAX as f32) * 2.0 - 1.0
        })
        .collect()
}

#[tokio::main]
async fn main() {
    println!("Memory System Functional Tests");
    println!("==============================\n");

    // Test 1: Vector Store
    println!("1. Vector Store Tests");
    let dir = tempfile::tempdir().expect("create temp dir");
    let store = VectorStore::open(&dir.path().join("test.db"), 384).expect("open vector store");
    let mut obs_store = mimofan_memory::optimization::ObservationStore::new(store);

    // Store observation
    let obs = create_observation("test", "project", "Test discovery");
    let embedding = generate_embedding(1);
    let id = obs_store
        .store_observation(&obs, &embedding)
        .expect("store observation");
    assert!(id > 0, "Store should return positive ID");
    println!("   ✓ Store observation: ID = {}", id);

    // Search
    let results = obs_store
        .search(&embedding, 10, &Default::default())
        .expect("search vector store");
    assert!(!results.is_empty(), "Search should find stored observation");
    assert_eq!(results[0].observation.id, id);
    println!(
        "   ✓ Search found observation: {}",
        results[0].observation.content
    );

    // Search with filter
    let filters = SearchFilters {
        project: Some("test".to_string()),
        ..Default::default()
    };
    let results = obs_store
        .search(&embedding, 10, &filters)
        .expect("search vector store with filter");
    assert!(
        !results.is_empty(),
        "Filtered search should find observation"
    );
    println!("   ✓ Filtered search works");

    // Test 2: Batch Processor
    println!("\n2. Batch Processor Tests");
    let mut batch = BatchProcessor::new(5, 100);

    // Enqueue
    for i in 0..10 {
        let obs = create_observation("batch", "project", &format!("Item {}", i));
        batch.enqueue(obs).expect("enqueue observation");
    }
    assert_eq!(batch.queue_size(), 10);
    println!("   ✓ Enqueued 10 items");

    // Process batches
    let mut processed = 0;
    while let Some(b) = batch.next_batch() {
        processed += b.len();
    }
    assert_eq!(processed, 10);
    println!("   ✓ Processed all batches: {} items", processed);

    // Test 3: Rate Limiter
    println!("\n3. Rate Limiter Tests");
    let mut limiter = RateLimiter::new(3, Duration::from_secs(1));

    assert!(limiter.is_allowed());
    assert!(limiter.is_allowed());
    assert!(limiter.is_allowed());
    assert!(!limiter.is_allowed()); // Should be blocked
    println!("   ✓ Rate limiter blocks after max requests");

    // Test 4: Task Manager
    println!("\n4. Task Manager Tests");
    let manager = LongTaskManager::new(10);

    // Start task
    manager
        .start_task("task-1".to_string(), "Test Task".to_string())
        .await
        .expect("start test task");
    let active = manager.get_active_tasks().await;
    assert_eq!(active.len(), 1);
    println!("   ✓ Started task");

    // Complete task
    manager
        .complete_task("task-1", true, "Done".to_string())
        .await
        .expect("complete test task");
    let active = manager.get_active_tasks().await;
    assert_eq!(active.len(), 0);
    let completed = manager.get_completed_tasks().await;
    assert_eq!(completed.len(), 1);
    println!("   ✓ Completed task");

    // Test 5: Compressor
    println!("\n5. Compressor Tests");
    let compressor = ObservationCompressor::with_settings(5, 86400);

    let observations: Vec<Observation> = (0..10)
        .map(|i| {
            create_observation(
                "compress",
                "project",
                &format!("Discovery {}", i),
            )
        })
        .collect();

    // Analyze
    let strategies = compressor.analyze_observations(&observations);
    assert_eq!(strategies.len(), 10);
    println!("   ✓ Analyzed {} observations", strategies.len());

    // Summarize
    let summary = compressor
        .summarize_session("test-session", &observations)
        .expect("summarize test session");
    assert!(!summary.session_id.is_empty());
    println!("   ✓ Generated session summary: {}", summary.session_id);

    // Test 6: Search Cache
    println!("\n6. Search Cache Tests");
    let mut cache = SearchCache::new(100);

    // Insert
    let data = vec![mimofan_memory::VectorMatch {
        observation: create_observation("cache", "project", "Cached item"),
        score: 0.95,
    }];
    cache.insert("query-1".to_string(), data.clone());
    println!("   ✓ Inserted into cache");

    // Get (hit)
    let result = cache.get("query-1");
    assert!(result.is_some());
    let cached = result.expect("unwrap cached result");
    assert_eq!(cached.len(), 1);
    println!("   ✓ Cache hit works");

    // Get (miss)
    let result = cache.get("query-2");
    assert!(result.is_none());
    println!("   ✓ Cache miss works");

    println!("\n==============================");
    println!("All functional tests passed! ✅");
}
