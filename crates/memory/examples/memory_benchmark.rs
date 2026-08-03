//! Memory System Benchmark
//!
//! Targeted tests for mimofan memory system capabilities:
//! 1. Vector store: CRUD operations and search
//! 2. Embedding: API performance
//! 3. Compressor: Observation compression
//! 4. Injector: Cross-session memory injection
//! 5. Knowledge: Corpus build and query
//! 6. Optimization: Batch processing, caching, rate limiting

use std::time::{Duration, Instant};

use mimofan_memory::compressor::ObservationCompressor;
use mimofan_memory::optimization::{BatchProcessor, LongTaskManager, RateLimiter};
use mimofan_memory::vector::{Observation, ObservationKind, SearchFilters, VectorStore};

/// Benchmark results
struct BenchmarkResult {
    name: String,
    iterations: usize,
    avg_duration: Duration,
    ops_per_sec: f64,
}

impl BenchmarkResult {
    fn new(name: &str, iterations: usize, total_duration: Duration) -> Self {
        let avg_duration = total_duration / iterations as u32;
        let ops_per_sec = iterations as f64 / total_duration.as_secs_f64();
        Self {
            name: name.to_string(),
            iterations,
            avg_duration,
            ops_per_sec,
        }
    }

    fn print(&self) {
        println!(
            "{:<30} {:>8} ops  {:>10.2?}/op  {:>10.2} ops/s",
            self.name, self.iterations, self.avg_duration, self.ops_per_sec
        );
    }
}

/// Generate test embedding (simple hash-based, not real embedding)
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

/// Benchmark 1: Vector Store Operations
fn benchmark_vector_store() {
    println!("\n=== Vector Store Benchmark ===");

    let dir = tempfile::tempdir().expect("create temp dir");
    let store = VectorStore::open(&dir.path().join("bench.db"), 384).expect("open vector store");

    // Benchmark: Store observations
    let iterations = 1000_usize;
    let start = Instant::now();
    for i in 0..iterations {
        let obs = Observation::new(
            "bench-project".to_string(),
            ObservationKind::Discovery,
            format!("Benchmark observation {}", i),
        );
        let embedding = generate_embedding(i as u64);
        store
            .store_observation(&obs, &embedding)
            .expect("store observation");
    }
    BenchmarkResult::new("Store observations", iterations, start.elapsed()).print();

    // Benchmark: Search
    let iterations = 100_usize;
    let query_embedding = generate_embedding(0);
    let start = Instant::now();
    for _ in 0..iterations {
        let _results = store
            .search(&query_embedding, 10, &Default::default())
            .expect("search vector store");
    }
    BenchmarkResult::new("Search (k=10)", iterations, start.elapsed()).print();

    // Benchmark: Search with filters
    let filters = SearchFilters {
        project: Some("bench-project".to_string()),
        kind: Some(ObservationKind::Discovery),
        ..Default::default()
    };
    let iterations = 100_usize;
    let start = Instant::now();
    for _ in 0..iterations {
        let _results = store
            .search(&query_embedding, 10, &filters)
            .expect("search vector store with filters");
    }
    BenchmarkResult::new("Search with filters", iterations, start.elapsed()).print();
}

/// Benchmark 2: Batch Processor
fn benchmark_batch_processor() {
    println!("\n=== Batch Processor Benchmark ===");

    let mut processor = BatchProcessor::new(100, 10_000);

    // Benchmark: Enqueue
    let iterations = 10_000_usize;
    let start = Instant::now();
    for i in 0..iterations {
        let obs = Observation::new(
            "batch-project".to_string(),
            ObservationKind::Change,
            format!("Batch observation {}", i),
        );
        processor.enqueue(obs).expect("enqueue observation");
    }
    BenchmarkResult::new("Enqueue 10k observations", iterations, start.elapsed()).print();

    // Benchmark: Process batches
    let start = Instant::now();
    let mut processed = 0;
    while let Some(batch) = processor.next_batch() {
        processed += batch.len();
    }
    BenchmarkResult::new("Process all batches", processed, start.elapsed()).print();
}

/// Benchmark 3: Rate Limiter
fn benchmark_rate_limiter() {
    println!("\n=== Rate Limiter Benchmark ===");

    let mut limiter = RateLimiter::new(1000, Duration::from_secs(1));

    // Benchmark: Check rate limit
    let iterations = 10_000_usize;
    let start = Instant::now();
    for _ in 0..iterations {
        limiter.is_allowed();
    }
    BenchmarkResult::new("Rate limit checks", iterations, start.elapsed()).print();
}

/// Benchmark 4: Task Manager
async fn benchmark_task_manager() {
    println!("\n=== Task Manager Benchmark ===");

    let manager = LongTaskManager::new(1000);

    // Benchmark: Start tasks
    let iterations = 1000_usize;
    let start = Instant::now();
    for i in 0..iterations {
        manager
            .start_task(format!("task-{}", i), format!("Task {}", i))
            .await
            .expect("start benchmark task");
    }
    BenchmarkResult::new("Start 1000 tasks", iterations, start.elapsed()).print();

    // Benchmark: Complete tasks
    let start = Instant::now();
    for i in 0..iterations {
        manager
            .complete_task(&format!("task-{}", i), true, "Done".to_string())
            .await
            .expect("complete benchmark task");
    }
    BenchmarkResult::new("Complete 1000 tasks", iterations, start.elapsed()).print();
}

/// Benchmark 5: Compression
fn benchmark_compressor() {
    println!("\n=== Compressor Benchmark ===");

    let compressor = ObservationCompressor::with_settings(10, 86400);

    // Generate test observations
    let observations: Vec<Observation> = (0..1000)
        .map(|i| {
            Observation::new(
                "compress-project".to_string(),
                ObservationKind::Discovery,
                format!("Compression test observation {}", i),
            )
        })
        .collect();

    // Benchmark: Analyze observations
    let iterations = 100_usize;
    let start = Instant::now();
    for _ in 0..iterations {
        let _strategies = compressor.analyze_observations(&observations);
    }
    BenchmarkResult::new("Analyze 1000 observations", iterations, start.elapsed()).print();

    // Benchmark: Summarize session
    let iterations = 100_usize;
    let start = Instant::now();
    for _ in 0..iterations {
        let _summary = compressor.summarize_session("test-session", &observations);
    }
    BenchmarkResult::new("Summarize session", iterations, start.elapsed()).print();
}

/// Benchmark 6: Search Cache
fn benchmark_search_cache() {
    println!("\n=== Search Cache Benchmark ===");

    let mut cache = mimofan_memory::optimization::SearchCache::new(1000);

    // Generate test data
    let test_data: Vec<(String, Vec<mimofan_memory::VectorMatch>)> = (0..1000)
        .map(|i| (format!("query-{}", i), Vec::new()))
        .collect();

    // Benchmark: Insert into cache
    let iterations = 1000_usize;
    let start = Instant::now();
    for (key, value) in &test_data {
        cache.insert(key.clone(), value.clone());
    }
    BenchmarkResult::new("Cache insert 1000 entries", iterations, start.elapsed()).print();

    // Benchmark: Cache hit
    let iterations = 1000_usize;
    let start = Instant::now();
    for i in 0..iterations {
        let _result = cache.get(&format!("query-{}", i % 1000));
    }
    BenchmarkResult::new("Cache get 1000 entries", iterations, start.elapsed()).print();
}

#[tokio::main]
async fn main() {
    println!("Mimofan Memory System Benchmark");
    println!("===============================");

    benchmark_vector_store();
    benchmark_batch_processor();
    benchmark_rate_limiter();
    benchmark_task_manager().await;
    benchmark_compressor();
    benchmark_search_cache();

    println!("\n===============================");
    println!("Benchmark completed!");
}
