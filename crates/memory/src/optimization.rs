//! Performance optimization for long-running tasks

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::{Mutex, RwLock};
use tracing::{debug, info};

use crate::error::MemoryError;
use crate::vector::{Observation, SearchFilters, VectorMatch, VectorStore};
use crate::Result;

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

/// LRU cache for vector search results
pub struct SearchCache {
    capacity: usize,
    cache: VecDeque<(String, Vec<VectorMatch>)>,
}

impl SearchCache {
    /// Create a new search cache
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            cache: VecDeque::new(),
        }
    }

    /// Get cached results
    pub fn get(&mut self, key: &str) -> Option<&Vec<VectorMatch>> {
        if let Some(pos) = self.cache.iter().position(|(k, _)| k == key) {
            // Move to back (most recently used)
            let item = self.cache.remove(pos).unwrap();
            self.cache.push_back(item);
            return self.cache.back().map(|(_, v)| v);
        }
        None
    }

    /// Insert into cache
    pub fn insert(&mut self, key: String, value: Vec<VectorMatch>) {
        if self.cache.len() >= self.capacity {
            self.cache.pop_front();
        }
        self.cache.push_back((key, value));
    }

    /// Clear cache
    pub fn clear(&mut self) {
        self.cache.clear();
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
            return Err(MemoryError::InvalidConfig("Too many active tasks".to_string()));
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
    pub fn store_observation(&mut self, observation: &Observation, embedding: &[f32]) -> Result<i64> {
        if !self.rate_limiter.is_allowed() {
            return Err(MemoryError::InvalidConfig("Rate limit exceeded".to_string()));
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
        let cache_key = format!(
            "{:?}:{:?}:{:?}",
            query_embedding.len(),
            limit,
            filters
        );

        // Check cache
        if let Some(cached) = self.search_cache.get(&cache_key) {
            debug!("Cache hit for search query");
            return Ok(cached.clone());
        }

        // Perform search
        let results = self.store.search(query_embedding, limit, filters)?;

        // Cache results
        self.search_cache.insert(cache_key, results.clone());

        Ok(results)
    }

    /// Process batch of observations
    pub async fn process_batch<F, Fut>(
        &mut self,
        processor: F,
    ) -> Result<usize>
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

    #[test]
    fn test_batch_processor() {
        let mut processor = BatchProcessor::new(2, 10);

        assert_eq!(processor.queue_size(), 0);
        assert!(processor.next_batch().is_none());
    }

    #[test]
    fn test_rate_limiter() {
        let mut limiter = RateLimiter::new(2, Duration::from_secs(1));

        assert!(limiter.is_allowed());
        assert!(limiter.is_allowed());
        assert!(!limiter.is_allowed());
    }

    #[tokio::test]
    async fn test_task_manager() {
        let manager = LongTaskManager::new(2);

        manager
            .start_task("1".to_string(), "Test".to_string())
            .await
            .unwrap();

        let active = manager.get_active_tasks().await;
        assert_eq!(active.len(), 1);

        manager
            .complete_task("1", true, "Done".to_string())
            .await
            .unwrap();

        let active = manager.get_active_tasks().await;
        assert_eq!(active.len(), 0);
    }
}
