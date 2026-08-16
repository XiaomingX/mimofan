//! Performance optimization for long-running tasks

use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::{Mutex, RwLock};
use tracing::debug;

use crate::Result;
use crate::error::MemoryError;
use crate::vector::{Observation, SearchFilters, VectorMatch, VectorStore};

/// Batch processor for efficient bulk operations
pub struct BatchProcessor {
    batch_size: usize,
    queue: VecDeque<Observation>,
    max_queue_size: usize,
}

impl BatchProcessor {
    /// Create a new batch processor
    pub fn new(batch_size: usize, max_queue_size: usize) -> Self {
        Self {
            batch_size,
            queue: VecDeque::new(),
            max_queue_size,
        }
    }

    /// Add observation to queue
    pub fn enqueue(&mut self, observation: Observation) -> Result<()> {
        if self.queue.len() >= self.max_queue_size {
            return Err(MemoryError::InvalidConfig("Batch queue full".to_string()));
        }
        self.queue.push_back(observation);
        Ok(())
    }

    /// Process next batch
    pub fn next_batch(&mut self) -> Option<Vec<Observation>> {
        if self.queue.is_empty() {
            return None;
        }

        let mut batch = Vec::with_capacity(self.batch_size);
        for _ in 0..self.batch_size {
            if let Some(obs) = self.queue.pop_front() {
                batch.push(obs);
            } else {
                break;
            }
        }

        Some(batch)
    }

    /// Get queue size
    pub fn queue_size(&self) -> usize {
        self.queue.len()
    }
}

/// LRU cache for vector search results.
///
/// Internally uses a [`RefCell`]-wrapped [`VecDeque`] so the cache can be
/// shared behind a `&self` borrow. This matches the `VectorMemory` access
/// pattern, where `&VectorMemory` is not `Send`/`Sync` (the inner
/// `VectorStore` holds non-`Send` SQLite/HNSW state), so the cache is only
/// ever used from a single thread and a `RefCell` is sufficient — wrapping it
/// in a `Mutex` would be unnecessary overhead and would not change the
/// (non-`Sync`) soundness profile.
///
/// The original `&mut self` methods are retained for the `examples/` and
/// `tests/` that drive the cache directly.
pub struct SearchCache {
    capacity: usize,
    cache: RefCell<VecDeque<(String, Vec<VectorMatch>)>>,
}

impl SearchCache {
    /// Create a new search cache
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            cache: RefCell::new(VecDeque::new()),
        }
    }

    /// Get cached results (shared borrow).
    ///
    /// Returns a clone of the cached matches on hit. The LRU position is
    /// updated (the matched entry is moved to the back) to reflect recency.
    pub fn get(&self, key: &str) -> Option<Vec<VectorMatch>> {
        let mut cache = self.cache.borrow_mut();
        if let Some(pos) = cache.iter().position(|(k, _)| k == key) {
            // Move to back (most recently used)
            let item = cache
                .remove(pos)
                .expect("cached search entry exists at position");
            cache.push_back(item.clone());
            return cache.back().map(|(_, v)| v.clone());
        }
        None
    }

    /// Insert into cache (shared borrow).
    pub fn insert(&self, key: String, value: Vec<VectorMatch>) {
        let mut cache = self.cache.borrow_mut();
        if cache.len() >= self.capacity {
            cache.pop_front();
        }
        cache.push_back((key, value));
    }

    /// Clear cache (shared borrow).
    pub fn clear(&self) {
        self.cache.borrow_mut().clear();
    }

    /// Number of entries currently cached.
    pub fn len(&self) -> usize {
        self.cache.borrow().len()
    }

    /// Whether the cache is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.cache.borrow().is_empty()
    }

    // ---- `&mut self` API retained for backward compatibility (examples/tests) ----

    /// Get cached results (mutable borrow, returns a reference).
    pub fn get_mut(&mut self, key: &str) -> Option<&Vec<VectorMatch>> {
        let cache = self.cache.get_mut();
        if let Some(pos) = cache.iter().position(|(k, _)| k == key) {
            let item = cache.remove(pos).expect("remove cached search result");
            cache.push_back(item);
            return cache.back().map(|(_, v)| v);
        }
        None
    }

    /// Insert into cache (mutable borrow).
    pub fn insert_mut(&mut self, key: String, value: Vec<VectorMatch>) {
        let cache = self.cache.get_mut();
        if cache.len() >= self.capacity {
            cache.pop_front();
        }
        cache.push_back((key, value));
    }

    /// Clear cache (mutable borrow).
    pub fn clear_mut(&mut self) {
        self.cache.get_mut().clear();
    }
}

/// Rate limiter for API calls
pub struct RateLimiter {
    max_requests: usize,
    window: Duration,
    requests: VecDeque<Instant>,
}

impl RateLimiter {
    /// Create a new rate limiter
    pub fn new(max_requests: usize, window: Duration) -> Self {
        Self {
            max_requests,
            window,
            requests: VecDeque::new(),
        }
    }

    /// Check if request is allowed
    pub fn is_allowed(&mut self) -> bool {
        let now = Instant::now();

        // Remove old requests outside window
        while let Some(&front) = self.requests.front() {
            if now.duration_since(front) > self.window {
                self.requests.pop_front();
            } else {
                break;
            }
        }

        if self.requests.len() < self.max_requests {
            self.requests.push_back(now);
            true
        } else {
            false
        }
    }
}

/// Long-running task manager
pub struct LongTaskManager {
    active_tasks: Arc<RwLock<Vec<LongTask>>>,
    completed_tasks: Arc<Mutex<VecDeque<LongTaskResult>>>,
    max_active_tasks: usize,
}

/// Long-running task
#[derive(Debug, Clone)]
pub struct LongTask {
    pub id: String,
    pub name: String,
    pub started_at: Instant,
    pub progress: f32, // 0.0 to 1.0
}

/// Task completion result
#[derive(Debug, Clone)]
pub struct LongTaskResult {
    pub id: String,
    pub name: String,
    pub success: bool,
    pub duration: Duration,
    pub message: String,
}

impl LongTaskManager {
    /// Create a new task manager
    pub fn new(max_active_tasks: usize) -> Self {
        Self {
            active_tasks: Arc::new(RwLock::new(Vec::new())),
            completed_tasks: Arc::new(Mutex::new(VecDeque::new())),
            max_active_tasks,
        }
    }

    /// Start a new task
    pub async fn start_task(&self, id: String, name: String) -> Result<()> {
        let mut tasks = self.active_tasks.write().await;
        if tasks.len() >= self.max_active_tasks {
            return Err(MemoryError::InvalidConfig(
                "Too many active tasks".to_string(),
            ));
        }

        tasks.push(LongTask {
            id,
            name,
            started_at: Instant::now(),
            progress: 0.0,
        });

        Ok(())
    }

    /// Update task progress
    pub async fn update_progress(&self, id: &str, progress: f32) -> Result<()> {
        let mut tasks = self.active_tasks.write().await;
        if let Some(task) = tasks.iter_mut().find(|t| t.id == id) {
            task.progress = progress.clamp(0.0, 1.0);
        }
        Ok(())
    }

    /// Complete a task
    pub async fn complete_task(&self, id: &str, success: bool, message: String) -> Result<()> {
        let mut tasks = self.active_tasks.write().await;
        if let Some(pos) = tasks.iter().position(|t| t.id == id) {
            let task = tasks.remove(pos);
            let duration = task.started_at.elapsed();

            let mut completed = self.completed_tasks.lock().await;
            completed.push_back(LongTaskResult {
                id: task.id,
                name: task.name,
                success,
                duration,
                message,
            });

            // Keep only last 100 completed tasks
            while completed.len() > 100 {
                completed.pop_front();
            }
        }
        Ok(())
    }

    /// Get active tasks
    pub async fn get_active_tasks(&self) -> Vec<LongTask> {
        self.active_tasks.read().await.clone()
    }

    /// Get recent completed tasks
    pub async fn get_completed_tasks(&self) -> Vec<LongTaskResult> {
        self.completed_tasks.lock().await.iter().cloned().collect()
    }
}

/// Memory-efficient observation store with compression
pub struct ObservationStore {
    store: VectorStore,
    batch_processor: BatchProcessor,
    search_cache: SearchCache,
    rate_limiter: RateLimiter,
}

impl ObservationStore {
    /// Create a new optimized observation store
    pub fn new(store: VectorStore) -> Self {
        Self {
            store,
            batch_processor: BatchProcessor::new(100, 10_000),
            search_cache: SearchCache::new(1_000),
            rate_limiter: RateLimiter::new(100, Duration::from_secs(60)),
        }
    }

    /// Store observation with rate limiting
    pub fn store_observation(
        &mut self,
        observation: &Observation,
        embedding: &[f32],
    ) -> Result<i64> {
        if !self.rate_limiter.is_allowed() {
            return Err(MemoryError::InvalidConfig(
                "Rate limit exceeded".to_string(),
            ));
        }

        self.store.store_observation(observation, embedding)
    }

    /// Search with caching
    pub fn search(
        &mut self,
        query_embedding: &[f32],
        limit: usize,
        filters: &SearchFilters,
    ) -> Result<Vec<VectorMatch>> {
        // Generate cache key
        let cache_key = format!("{:?}:{:?}:{:?}", query_embedding.len(), limit, filters);

        // Check cache
        if let Some(cached) = self.search_cache.get(&cache_key) {
            debug!("Cache hit for search query");
            return Ok(cached);
        }

        // Perform search
        let results = self.store.search(query_embedding, limit, filters)?;

        // Cache results
        self.search_cache.insert(cache_key, results.clone());

        Ok(results)
    }

    /// Process batch of observations
    pub async fn process_batch<F, Fut>(&mut self, processor: F) -> Result<usize>
    where
        F: Fn(Observation) -> Fut,
        Fut: std::future::Future<Output = Result<()>>,
    {
        let mut processed = 0;

        while let Some(batch) = self.batch_processor.next_batch() {
            for obs in batch {
                processor(obs).await?;
                processed += 1;
            }
        }

        Ok(processed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
