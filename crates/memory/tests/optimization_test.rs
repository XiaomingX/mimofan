use mimofan_memory::*;
use std::time::Duration;

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
        .expect("start test task");

    let active = manager.get_active_tasks().await;
    assert_eq!(active.len(), 1);

    manager
        .complete_task("1", true, "Done".to_string())
        .await
        .expect("complete test task");

    let active = manager.get_active_tasks().await;
    assert_eq!(active.len(), 0);
}
