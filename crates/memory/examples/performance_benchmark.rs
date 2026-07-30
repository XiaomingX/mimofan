//! Performance benchmark for memory system

use mimofan_memory::vector::{Observation, ObservationKind, VectorStore};
use mimofan_memory::optimization::{BatchProcessor, LongTaskManager, ObservationStore, RateLimiter};
use mimofan_memory::compressor::ObservationCompressor;
use std::time::Duration;

#[tokio::main]
async fn main() {
    println!("Memory System Performance Benchmark");
    println!("===================================\n");

    // Benchmark 1: Vector Store Performance
    println!("1. Vector Store Performance:");
    let dir = tempfile::tempdir().unwrap();
    let store = VectorStore::open(&dir.path().join("benchmark.db"), 384).unwrap();

    let mut observation_store = ObservationStore::new(store);

    // Generate test observations
    let start = std::time::Instant::now();
    let embeddings: Vec<Vec<f32>> = (0..1000)
        .map(|_| (0..384).map(|_| rand::random::<f32>()).collect())
        .collect();

    for i in 0..1000 {
        let obs = Observation::new(
            "benchmark-project".to_string(),
            ObservationKind::Discovery,
            format!("Benchmark observation {}", i),
        );
        observation_store.store_observation(&obs, &embeddings[i]).unwrap();
    }
    println!("   Store 1000 observations: {:?}", start.elapsed());

    // Benchmark 2: Search Performance
    let start = std::time::Instant::now();
    let query = (0..384).map(|_| rand::random::<f32>()).collect::<Vec<f32>>();
    for _ in 0..100 {
        let _results = observation_store.search(&query, 10, &Default::default()).unwrap();
    }
    println!("   100 search queries: {:?}", start.elapsed());

    // Benchmark 3: Batch Processor
    println!("\n2. Batch Processor Performance:");
    let mut batch_proc = BatchProcessor::new(100, 10_000);

    let start = std::time::Instant::now();
    for i in 0..10_000 {
        let obs = Observation::new(
            "batch-project".to_string(),
            ObservationKind::Change,
            format!("Batch observation {}", i),
        );
        batch_proc.enqueue(obs).unwrap();
    }
    println!("   Enqueue 10,000 observations: {:?}", start.elapsed());

    let start = std::time::Instant::now();
    while batch_proc.next_batch().is_some() {}
    println!("   Process all batches: {:?}", start.elapsed());

    // Benchmark 4: Task Manager
    println!("\n3. Long Task Manager Performance:");
    let manager = LongTaskManager::new(100);

    let start = std::time::Instant::now();
    for i in 0..100 {
        manager
            .start_task(format!("task-{}", i), format!("Task {}", i))
            .await
            .unwrap();
    }
    println!("   Start 100 tasks: {:?}", start.elapsed());

    let start = std::time::Instant::now();
    for i in 0..100 {
        manager
            .complete_task(&format!("task-{}", i), true, "Done".to_string())
            .await
            .unwrap();
    }
    println!("   Complete 100 tasks: {:?}", start.elapsed());

    // Benchmark 5: Rate Limiter
    println!("\n4. Rate Limiter Performance:");
    let mut limiter = RateLimiter::new(1000, Duration::from_secs(1));

    let start = std::time::Instant::now();
    for _ in 0..10_000 {
        limiter.is_allowed();
    }
    println!("   10,000 rate limit checks: {:?}", start.elapsed());

    println!("\n===================================");
    println!("Benchmark completed!");
}
