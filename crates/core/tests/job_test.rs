use mimofan_core::*;
use mimofan_state::JobStateStatus;
use serde_json::{Value, json};

#[test]
fn enqueue_creates_queued_job_with_zero_progress() {
    let mut jm = JobManager::default();
    let job = jm.enqueue("build");
    assert_eq!(job.name, "build");
    assert_eq!(job.status, JobStatus::Queued);
    assert_eq!(job.progress, Some(0));
    assert!(job.detail.is_none());
    assert_eq!(job.history.len(), 1);
    assert_eq!(job.history[0].phase, "created");
}

#[test]
fn set_running_transitions_from_queued() {
    let mut jm = JobManager::default();
    let job = jm.enqueue("deploy");
    let id = job.id.clone();
    jm.set_running(&id);
    let jobs = jm.list();
    let updated = jobs
        .iter()
        .find(|j| j.id == id)
        .expect("find updated job by id");
    assert_eq!(updated.status, JobStatus::Running);
    assert_eq!(
        updated
            .history
            .last()
            .expect("job history has last entry")
            .phase,
        "running"
    );
}

#[test]
fn update_progress_clamps_to_100() {
    let mut jm = JobManager::default();
    let job = jm.enqueue("task");
    let id = job.id.clone();
    jm.update_progress(&id, 150, Some("over".to_string()));
    let jobs = jm.list();
    let updated = jobs
        .iter()
        .find(|j| j.id == id)
        .expect("find updated job by id");
    assert_eq!(updated.progress, Some(100));
}

#[test]
fn complete_sets_progress_to_100() {
    let mut jm = JobManager::default();
    let job = jm.enqueue("task");
    let id = job.id.clone();
    jm.set_running(&id);
    jm.complete(&id);
    let jobs = jm.list();
    let updated = jobs
        .iter()
        .find(|j| j.id == id)
        .expect("find updated job by id");
    assert_eq!(updated.status, JobStatus::Completed);
    assert_eq!(updated.progress, Some(100));
}

#[test]
fn fail_increments_attempt_and_sets_backoff() {
    let mut jm = JobManager::default();
    let job = jm.enqueue("fragile");
    let id = job.id.clone();
    jm.set_running(&id);
    jm.fail(&id, "crashed");
    let jobs = jm.list();
    let updated = jobs
        .iter()
        .find(|j| j.id == id)
        .expect("find updated job by id");
    assert_eq!(updated.status, JobStatus::Failed);
    assert_eq!(updated.retry.attempt, 1);
    assert!(updated.retry.next_backoff_ms > 0);
    assert!(updated.retry.next_retry_at.is_some());
    assert_eq!(updated.detail.as_deref(), Some("crashed"));
}

#[test]
fn fail_clears_retry_after_max_attempts() {
    let mut jm = JobManager::default();
    let job = jm.enqueue("fragile");
    let id = job.id.clone();
    for _ in 0..=DEFAULT_JOB_MAX_ATTEMPTS {
        jm.set_running(&id);
        jm.fail(&id, "boom");
    }
    let jobs = jm.list();
    let updated = jobs
        .iter()
        .find(|j| j.id == id)
        .expect("find updated job by id");
    assert_eq!(updated.retry.attempt, DEFAULT_JOB_MAX_ATTEMPTS);
    assert_eq!(updated.retry.next_backoff_ms, 0);
    assert!(updated.retry.next_retry_at.is_none());
}

#[test]
fn cancel_sets_status_and_clears_retry() {
    let mut jm = JobManager::default();
    let job = jm.enqueue("task");
    let id = job.id.clone();
    jm.cancel(&id);
    let jobs = jm.list();
    let updated = jobs
        .iter()
        .find(|j| j.id == id)
        .expect("find updated job by id");
    assert_eq!(updated.status, JobStatus::Cancelled);
    assert_eq!(updated.retry.next_backoff_ms, 0);
}

#[test]
fn pause_and_resume_round_trip() {
    let mut jm = JobManager::default();
    let job = jm.enqueue("task");
    let id = job.id.clone();
    jm.set_running(&id);
    jm.pause(&id, Some("waiting".to_string()));
    let jobs = jm.list();
    let paused = jobs
        .iter()
        .find(|j| j.id == id)
        .expect("find paused job by id");
    assert_eq!(paused.status, JobStatus::Paused);
    assert_eq!(paused.detail.as_deref(), Some("waiting"));

    jm.resume(&id, None);
    let jobs = jm.list();
    let resumed = jobs
        .iter()
        .find(|j| j.id == id)
        .expect("find resumed job by id");
    assert_eq!(resumed.status, JobStatus::Running);
    assert_eq!(
        resumed
            .history
            .last()
            .expect("resumed job history has last entry")
            .phase,
        "resumed"
    );
}

#[test]
fn list_returns_jobs_sorted_by_updated_at_desc() {
    let mut jm = JobManager::default();
    jm.enqueue("first");
    jm.enqueue("second");
    jm.enqueue("third");
    let jobs = jm.list();
    assert_eq!(jobs.len(), 3);
    for window in jobs.windows(2) {
        assert!(window[0].updated_at >= window[1].updated_at);
    }
}

#[test]
fn history_returns_entries_for_existing_job() {
    let mut jm = JobManager::default();
    let job = jm.enqueue("task");
    let id = job.id.clone();
    jm.set_running(&id);
    jm.complete(&id);
    let history = jm.history(&id);
    assert_eq!(history.len(), 3); // created, running, completed
    assert_eq!(history[0].phase, "created");
    assert_eq!(history[1].phase, "running");
    assert_eq!(history[2].phase, "completed");
}

#[test]
fn history_returns_empty_for_unknown_job() {
    let jm = JobManager::default();
    assert!(jm.history("nonexistent").is_empty());
}

#[test]
fn resume_pending_requeues_running_and_queued() {
    let mut jm = JobManager::default();
    let _j1 = jm.enqueue("queued_task");
    let j2 = jm.enqueue("running_task");
    let j3 = jm.enqueue("completed_task");
    let id2 = j2.id.clone();
    let id3 = j3.id.clone();
    jm.set_running(&id2);
    jm.set_running(&id3);
    jm.complete(&id3);

    let resumed = jm.resume_pending();
    assert_eq!(resumed.len(), 2);
    for job in &resumed {
        assert_eq!(job.status, JobStatus::Queued);
    }
}

// ── JobManager: backoff ────────────────────────────────────────────

#[test]
fn deterministic_backoff_zero_on_first_attempt() {
    let retry = JobRetryMetadata {
        attempt: 0,
        ..Default::default()
    };
    assert_eq!(JobManager::deterministic_backoff_ms(&retry), 0);
}

#[test]
fn deterministic_backoff_exponential_growth() {
    let base = DEFAULT_JOB_BACKOFF_BASE_MS;
    for attempt in 1..=5 {
        let retry = JobRetryMetadata {
            attempt,
            backoff_base_ms: base,
            ..Default::default()
        };
        let expected = base * 2u64.pow(attempt.saturating_sub(1).min(20));
        assert_eq!(
            JobManager::deterministic_backoff_ms(&retry),
            expected,
            "attempt {attempt}"
        );
    }
}

#[test]
fn deterministic_backoff_saturates_at_high_exponent() {
    let retry = JobRetryMetadata {
        attempt: 63,
        backoff_base_ms: 1000,
        ..Default::default()
    };
    // Should not panic; result saturates
    let _ = JobManager::deterministic_backoff_ms(&retry);
}

// ── JobManager: history truncation ─────────────────────────────────

#[test]
fn push_history_truncates_beyond_max() {
    let mut jm = JobManager::default();
    let job = jm.enqueue("task");
    let id = job.id.clone();
    // Generate more history entries than the limit
    for i in 0..(MAX_JOB_HISTORY_ENTRIES + 20) {
        jm.update_progress(&id, (i % 100) as u8, Some(format!("step {i}")));
    }
    let history = jm.history(&id);
    assert_eq!(history.len(), MAX_JOB_HISTORY_ENTRIES);
}

// ── JobManager: persistence encoding/parsing ───────────────────────

#[test]
fn encode_and_parse_persisted_detail_round_trip() {
    let mut jm = JobManager::default();
    let job = jm.enqueue("task");
    let id = job.id.clone();
    jm.set_running(&id);
    jm.fail(&id, "oops");
    let job = jm
        .list()
        .into_iter()
        .find(|j| j.id == id)
        .expect("find persisted job by id");

    let encoded = JobManager::encode_persisted_detail(&job)
        .expect("encode persisted job detail")
        .expect("persisted detail present");
    let parsed = JobManager::parse_persisted_detail(Some(&encoded))
        .expect("parse persisted detail round-trip");

    assert_eq!(parsed.status, job.status);
    assert_eq!(parsed.detail, job.detail);
    assert_eq!(parsed.retry.attempt, job.retry.attempt);
    assert_eq!(parsed.history.len(), job.history.len());
}

#[test]
fn parse_persisted_detail_returns_none_for_none_input() {
    assert!(JobManager::parse_persisted_detail(None).is_none());
}

#[test]
fn parse_persisted_detail_returns_none_for_invalid_json() {
    assert!(JobManager::parse_persisted_detail(Some("not json")).is_none());
}

// ── Helper functions ───────────────────────────────────────────────

#[test]
fn job_status_round_trip_str() {
    let statuses = [
        JobStatus::Queued,
        JobStatus::Running,
        JobStatus::Paused,
        JobStatus::Completed,
        JobStatus::Failed,
        JobStatus::Cancelled,
    ];
    for status in &statuses {
        let s = job_status_to_str(*status);
        let parsed = job_status_from_str(s);
        assert_eq!(parsed, Some(*status), "round-trip failed for {s:?}");
    }
}

#[test]
fn job_status_from_str_returns_none_for_unknown() {
    assert_eq!(job_status_from_str("unknown"), None);
    assert_eq!(job_status_from_str(""), None);
}

#[test]
fn runtime_status_to_job_state_maps_correctly() {
    assert_eq!(
        runtime_status_to_job_state(JobStatus::Queued),
        JobStateStatus::Queued
    );
    assert_eq!(
        runtime_status_to_job_state(JobStatus::Running),
        JobStateStatus::Running
    );
    assert_eq!(
        runtime_status_to_job_state(JobStatus::Paused),
        JobStateStatus::Running
    );
    assert_eq!(
        runtime_status_to_job_state(JobStatus::Completed),
        JobStateStatus::Completed
    );
    assert_eq!(
        runtime_status_to_job_state(JobStatus::Failed),
        JobStateStatus::Failed
    );
    assert_eq!(
        runtime_status_to_job_state(JobStatus::Cancelled),
        JobStateStatus::Cancelled
    );
}

#[test]
fn job_state_status_to_runtime_maps_correctly() {
    assert_eq!(
        job_state_status_to_runtime(JobStateStatus::Queued),
        JobStatus::Queued
    );
    assert_eq!(
        job_state_status_to_runtime(JobStateStatus::Running),
        JobStatus::Running
    );
    assert_eq!(
        job_state_status_to_runtime(JobStateStatus::Completed),
        JobStatus::Completed
    );
    assert_eq!(
        job_state_status_to_runtime(JobStateStatus::Failed),
        JobStatus::Failed
    );
    assert_eq!(
        job_state_status_to_runtime(JobStateStatus::Cancelled),
        JobStatus::Cancelled
    );
}

#[test]
fn json_optional_string_handles_null() {
    assert!(json_optional_string(&Value::Null).is_none());
}

#[test]
fn json_optional_string_handles_string() {
    assert_eq!(
        json_optional_string(&Value::String("hello".to_string())),
        Some("hello".to_string())
    );
}

#[test]
fn json_optional_string_handles_non_string() {
    assert!(json_optional_string(&json!(42)).is_none());
}

#[test]
fn parse_retry_metadata_returns_default_for_none() {
    let retry = parse_retry_metadata(None);
    assert_eq!(retry.attempt, 0);
    assert_eq!(retry.max_attempts, DEFAULT_JOB_MAX_ATTEMPTS);
    assert_eq!(retry.backoff_base_ms, DEFAULT_JOB_BACKOFF_BASE_MS);
}

#[test]
fn parse_retry_metadata_parses_fields() {
    let value = json!({
        "attempt": 2,
        "max_attempts": 5,
        "backoff_base_ms": 1000,
        "next_backoff_ms": 2000,
        "next_retry_at": 1234567890i64
    });
    let retry = parse_retry_metadata(Some(&value));
    assert_eq!(retry.attempt, 2);
    assert_eq!(retry.max_attempts, 5);
    assert_eq!(retry.backoff_base_ms, 1000);
    assert_eq!(retry.next_backoff_ms, 2000);
    assert_eq!(retry.next_retry_at, Some(1234567890));
}

#[test]
fn parse_history_entry_returns_none_without_status() {
    let value = json!({"at": 1, "phase": "test"});
    assert!(parse_history_entry(&value).is_none());
}
