use std::collections::HashMap;

use anyhow::Result;
use codewhale_state::{JobStateRecord, JobStateStatus, StateStore};
use serde_json::{Value, json};
use uuid::Uuid;

pub(crate) const JOB_DETAIL_SCHEMA_VERSION: u8 = 1;
pub(crate) const DEFAULT_JOB_MAX_ATTEMPTS: u32 = 3;
pub(crate) const DEFAULT_JOB_BACKOFF_BASE_MS: u64 = 500;
pub(crate) const MAX_JOB_HISTORY_ENTRIES: usize = 64;

/// Status of a background job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStatus {
    /// Waiting to be picked up.
    Queued,
    /// Currently executing.
    Running,
    /// Temporarily paused.
    Paused,
    /// Finished successfully.
    Completed,
    /// Finished with an error.
    Failed,
    /// Cancelled by the user.
    Cancelled,
}

/// Retry state for a job that failed and may be retried.
#[derive(Debug, Clone)]
pub struct JobRetryMetadata {
    /// Current attempt number (0 = not yet retried).
    pub attempt: u32,
    /// Maximum number of retry attempts before giving up.
    pub max_attempts: u32,
    /// Base delay in milliseconds for exponential backoff.
    pub backoff_base_ms: u64,
    /// Computed delay in milliseconds until the next retry.
    pub next_backoff_ms: u64,
    /// Timestamp when the next retry should be attempted.
    pub next_retry_at: Option<i64>,
}

impl Default for JobRetryMetadata {
    fn default() -> Self {
        Self {
            attempt: 0,
            max_attempts: DEFAULT_JOB_MAX_ATTEMPTS,
            backoff_base_ms: DEFAULT_JOB_BACKOFF_BASE_MS,
            next_backoff_ms: 0,
            next_retry_at: None,
        }
    }
}

/// A single entry in a job's history log.
#[derive(Debug, Clone)]
pub struct JobHistoryEntry {
    /// Timestamp when this entry was recorded.
    pub at: i64,
    /// Phase name (e.g., "created", "running", "failed").
    pub phase: String,
    /// Job status at this point in time.
    pub status: JobStatus,
    /// Progress percentage at this point, if available.
    pub progress: Option<u8>,
    /// Human-readable detail message.
    pub detail: Option<String>,
    /// Retry state snapshot at this point.
    pub retry: JobRetryMetadata,
}

#[derive(Debug, Clone)]
pub(crate) struct PersistedJobDetail {
    pub status: JobStatus,
    pub detail: Option<String>,
    pub retry: JobRetryMetadata,
    pub history: Vec<JobHistoryEntry>,
}

/// A complete job record with all metadata and history.
#[derive(Debug, Clone)]
pub struct JobRecord {
    /// Unique job identifier.
    pub id: String,
    /// Human-readable job name.
    pub name: String,
    /// Current job status.
    pub status: JobStatus,
    /// Current progress percentage (0-100).
    pub progress: Option<u8>,
    /// Human-readable detail about the current state.
    pub detail: Option<String>,
    /// Retry state for failed jobs.
    pub retry: JobRetryMetadata,
    /// Chronological history of state transitions.
    pub history: Vec<JobHistoryEntry>,
    /// Timestamp when the job was created.
    pub created_at: i64,
    /// Timestamp of the last state change.
    pub updated_at: i64,
}

/// Manages background jobs with retry logic and persistence.
#[derive(Debug, Default)]
pub struct JobManager {
    jobs: HashMap<String, JobRecord>,
}

impl JobManager {
    fn now_ts() -> i64 {
        chrono::Utc::now().timestamp()
    }

    fn deterministic_backoff_ms(retry: &JobRetryMetadata) -> u64 {
        if retry.attempt == 0 {
            return 0;
        }
        let exponent = retry.attempt.saturating_sub(1).min(20);
        let multiplier = 1u64.checked_shl(exponent).unwrap_or(u64::MAX);
        retry.backoff_base_ms.saturating_mul(multiplier)
    }

    fn clear_retry_schedule(retry: &mut JobRetryMetadata) {
        retry.next_backoff_ms = 0;
        retry.next_retry_at = None;
    }

    fn push_history(job: &mut JobRecord, phase: &str) {
        job.history.push(JobHistoryEntry {
            at: job.updated_at,
            phase: phase.to_string(),
            status: job.status,
            progress: job.progress,
            detail: job.detail.clone(),
            retry: job.retry.clone(),
        });
        if job.history.len() > MAX_JOB_HISTORY_ENTRIES {
            let to_drain = job.history.len() - MAX_JOB_HISTORY_ENTRIES;
            job.history.drain(0..to_drain);
        }
    }

    pub(crate) fn parse_persisted_detail(raw: Option<&str>) -> Option<PersistedJobDetail> {
        let raw = raw?;
        let parsed: Value = serde_json::from_str(raw).ok()?;
        let status = parsed
            .get("status")
            .and_then(Value::as_str)
            .and_then(job_status_from_str)?;
        let detail = parsed.get("detail").and_then(json_optional_string);
        let retry = parse_retry_metadata(parsed.get("retry"));
        let history = parsed
            .get("history")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(parse_history_entry)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        Some(PersistedJobDetail {
            status,
            detail,
            retry,
            history,
        })
    }

    pub(crate) fn encode_persisted_detail(job: &JobRecord) -> Result<Option<String>> {
        let encoded = json!({
            "schema_version": JOB_DETAIL_SCHEMA_VERSION,
            "status": job_status_to_str(job.status),
            "detail": job.detail.clone(),
            "retry": job_retry_to_value(&job.retry),
            "history": job.history.iter().map(job_history_to_value).collect::<Vec<_>>()
        })
        .to_string();
        Ok(Some(encoded))
    }

    /// Enqueues a new job and returns its record.
    pub fn enqueue(&mut self, name: impl Into<String>) -> JobRecord {
        let now = Self::now_ts();
        let id = format!("job-{}", Uuid::new_v4());
        let mut job = JobRecord {
            id: id.clone(),
            name: name.into(),
            status: JobStatus::Queued,
            progress: Some(0),
            detail: None,
            retry: JobRetryMetadata::default(),
            history: Vec::new(),
            created_at: now,
            updated_at: now,
        };
        Self::push_history(&mut job, "created");
        self.jobs.insert(id, job.clone());
        job
    }

    /// Transitions a job to running and clears its retry schedule.
    pub fn set_running(&mut self, id: &str) {
        if let Some(job) = self.jobs.get_mut(id) {
            job.status = JobStatus::Running;
            Self::clear_retry_schedule(&mut job.retry);
            job.updated_at = Self::now_ts();
            Self::push_history(job, "running");
        }
    }

    /// Updates a job's progress (clamped to 100) and optional detail message.
    pub fn update_progress(&mut self, id: &str, progress: u8, detail: Option<String>) {
        if let Some(job) = self.jobs.get_mut(id) {
            job.progress = Some(progress.min(100));
            job.detail = detail;
            job.updated_at = Self::now_ts();
            Self::push_history(job, "progress_updated");
        }
    }

    /// Marks a job as completed with 100% progress and clears its retry schedule.
    pub fn complete(&mut self, id: &str) {
        if let Some(job) = self.jobs.get_mut(id) {
            job.status = JobStatus::Completed;
            job.progress = Some(100);
            Self::clear_retry_schedule(&mut job.retry);
            job.updated_at = Self::now_ts();
            Self::push_history(job, "completed");
        }
    }

    /// Marks a job as failed and schedules a retry if attempts remain.
    pub fn fail(&mut self, id: &str, detail: impl Into<String>) {
        if let Some(job) = self.jobs.get_mut(id) {
            let now = Self::now_ts();
            job.status = JobStatus::Failed;
            job.detail = Some(detail.into());
            if job.retry.attempt < job.retry.max_attempts {
                job.retry.attempt += 1;
                job.retry.next_backoff_ms = Self::deterministic_backoff_ms(&job.retry);
                let delay_secs = ((job.retry.next_backoff_ms.saturating_add(999)) / 1000)
                    .min(i64::MAX as u64) as i64;
                job.retry.next_retry_at = Some(now.saturating_add(delay_secs));
            } else {
                Self::clear_retry_schedule(&mut job.retry);
            }
            job.updated_at = now;
            Self::push_history(job, "failed");
        }
    }

    /// Cancels a job and clears any pending retry schedule.
    pub fn cancel(&mut self, id: &str) {
        if let Some(job) = self.jobs.get_mut(id) {
            job.status = JobStatus::Cancelled;
            Self::clear_retry_schedule(&mut job.retry);
            job.updated_at = Self::now_ts();
            Self::push_history(job, "cancelled");
        }
    }

    /// Pauses a job, optionally updating its detail message.
    pub fn pause(&mut self, id: &str, detail: Option<String>) {
        if let Some(job) = self.jobs.get_mut(id) {
            job.status = JobStatus::Paused;
            if detail.is_some() {
                job.detail = detail;
            }
            job.updated_at = Self::now_ts();
            Self::push_history(job, "paused");
        }
    }

    /// Resumes a paused or failed job back to running status.
    pub fn resume(&mut self, id: &str, detail: Option<String>) {
        if let Some(job) = self.jobs.get_mut(id) {
            job.status = JobStatus::Running;
            if detail.is_some() {
                job.detail = detail;
            }
            Self::clear_retry_schedule(&mut job.retry);
            job.updated_at = Self::now_ts();
            Self::push_history(job, "resumed");
        }
    }

    /// Returns all jobs sorted by most recently updated first.
    pub fn list(&self) -> Vec<JobRecord> {
        let mut out = self.jobs.values().cloned().collect::<Vec<_>>();
        out.sort_by_key(|job| std::cmp::Reverse(job.updated_at));
        out
    }

    /// Returns the history entries for a job, or an empty vec if not found.
    pub fn history(&self, id: &str) -> Vec<JobHistoryEntry> {
        self.jobs
            .get(id)
            .map(|job| job.history.clone())
            .unwrap_or_default()
    }

    /// Resets queued or running jobs back to queued on application resume.
    pub fn resume_pending(&mut self) -> Vec<JobRecord> {
        let mut resumed = Vec::new();
        for job in self.jobs.values_mut() {
            if matches!(job.status, JobStatus::Queued | JobStatus::Running) {
                job.status = JobStatus::Queued;
                job.updated_at = Self::now_ts();
                Self::push_history(job, "queued_after_resume");
                resumed.push(job.clone());
            }
        }
        resumed
    }

    /// Loads jobs from the state store, deserializing extended detail when available.
    pub fn load_from_store(&mut self, store: &StateStore) -> Result<()> {
        let persisted = store.list_jobs(Some(500))?;
        for job in persisted {
            let fallback_status = job_state_status_to_runtime(job.status);
            let parsed = Self::parse_persisted_detail(job.detail.as_deref());
            let (status, detail, retry, history) = if let Some(detail_state) = parsed {
                (
                    detail_state.status,
                    detail_state.detail,
                    detail_state.retry,
                    detail_state.history,
                )
            } else {
                (
                    fallback_status,
                    job.detail,
                    JobRetryMetadata::default(),
                    Vec::new(),
                )
            };
            self.jobs.insert(
                job.id.clone(),
                JobRecord {
                    id: job.id,
                    name: job.name,
                    status,
                    progress: job.progress,
                    detail,
                    retry,
                    history,
                    created_at: job.created_at,
                    updated_at: job.updated_at,
                },
            );
        }
        Ok(())
    }

    /// Persists a single job's current state to the state store.
    pub fn persist_job(&self, store: &StateStore, id: &str) -> Result<()> {
        let Some(job) = self.jobs.get(id) else {
            return Ok(());
        };
        let encoded_detail = Self::encode_persisted_detail(job)?;
        store.upsert_job(&JobStateRecord {
            id: job.id.clone(),
            name: job.name.clone(),
            status: runtime_status_to_job_state(job.status),
            progress: job.progress,
            detail: encoded_detail,
            created_at: job.created_at,
            updated_at: job.updated_at,
        })
    }

    /// Persists all in-memory jobs to the state store.
    pub fn persist_all(&self, store: &StateStore) -> Result<()> {
        for id in self.jobs.keys() {
            self.persist_job(store, id)?;
        }
        Ok(())
    }
}

// ── Helper functions ──────────────────────────────────────────────────

pub(crate) fn job_status_to_str(status: JobStatus) -> &'static str {
    match status {
        JobStatus::Queued => "queued",
        JobStatus::Running => "running",
        JobStatus::Paused => "paused",
        JobStatus::Completed => "completed",
        JobStatus::Failed => "failed",
        JobStatus::Cancelled => "cancelled",
    }
}

pub(crate) fn job_status_from_str(value: &str) -> Option<JobStatus> {
    match value {
        "queued" => Some(JobStatus::Queued),
        "running" => Some(JobStatus::Running),
        "paused" => Some(JobStatus::Paused),
        "completed" => Some(JobStatus::Completed),
        "failed" => Some(JobStatus::Failed),
        "cancelled" => Some(JobStatus::Cancelled),
        _ => None,
    }
}

pub(crate) fn job_retry_to_value(retry: &JobRetryMetadata) -> Value {
    json!({
        "attempt": retry.attempt,
        "max_attempts": retry.max_attempts,
        "backoff_base_ms": retry.backoff_base_ms,
        "next_backoff_ms": retry.next_backoff_ms,
        "next_retry_at": retry.next_retry_at
    })
}

pub(crate) fn job_history_to_value(entry: &JobHistoryEntry) -> Value {
    json!({
        "at": entry.at,
        "phase": entry.phase.clone(),
        "status": job_status_to_str(entry.status),
        "progress": entry.progress,
        "detail": entry.detail.clone(),
        "retry": job_retry_to_value(&entry.retry)
    })
}

pub(crate) fn runtime_status_to_job_state(status: JobStatus) -> JobStateStatus {
    match status {
        JobStatus::Queued => JobStateStatus::Queued,
        JobStatus::Running => JobStateStatus::Running,
        JobStatus::Paused => JobStateStatus::Running,
        JobStatus::Completed => JobStateStatus::Completed,
        JobStatus::Failed => JobStateStatus::Failed,
        JobStatus::Cancelled => JobStateStatus::Cancelled,
    }
}

pub(crate) fn job_state_status_to_runtime(status: JobStateStatus) -> JobStatus {
    match status {
        JobStateStatus::Queued => JobStatus::Queued,
        JobStateStatus::Running => JobStatus::Running,
        JobStateStatus::Completed => JobStatus::Completed,
        JobStateStatus::Failed => JobStatus::Failed,
        JobStateStatus::Cancelled => JobStatus::Cancelled,
    }
}

pub(crate) fn json_optional_string(value: &Value) -> Option<String> {
    if value.is_null() {
        None
    } else {
        value.as_str().map(ToString::to_string)
    }
}

pub(crate) fn parse_retry_metadata(value: Option<&Value>) -> JobRetryMetadata {
    let Some(value) = value else {
        return JobRetryMetadata::default();
    };
    JobRetryMetadata {
        attempt: value
            .get("attempt")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            .min(u32::MAX as u64) as u32,
        max_attempts: value
            .get("max_attempts")
            .and_then(Value::as_u64)
            .unwrap_or(DEFAULT_JOB_MAX_ATTEMPTS as u64)
            .min(u32::MAX as u64) as u32,
        backoff_base_ms: value
            .get("backoff_base_ms")
            .and_then(Value::as_u64)
            .unwrap_or(DEFAULT_JOB_BACKOFF_BASE_MS),
        next_backoff_ms: value
            .get("next_backoff_ms")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        next_retry_at: value.get("next_retry_at").and_then(Value::as_i64),
    }
}

fn parse_history_entry(value: &Value) -> Option<JobHistoryEntry> {
    let status = value
        .get("status")
        .and_then(Value::as_str)
        .and_then(job_status_from_str)?;
    Some(JobHistoryEntry {
        at: value.get("at").and_then(Value::as_i64).unwrap_or(0),
        phase: value
            .get("phase")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string(),
        status,
        progress: value
            .get("progress")
            .and_then(Value::as_u64)
            .map(|v| v.min(u8::MAX as u64) as u8),
        detail: value.get("detail").and_then(json_optional_string),
        retry: parse_retry_metadata(value.get("retry")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let updated = jobs.iter().find(|j| j.id == id).unwrap();
        assert_eq!(updated.status, JobStatus::Running);
        assert_eq!(updated.history.last().unwrap().phase, "running");
    }

    #[test]
    fn update_progress_clamps_to_100() {
        let mut jm = JobManager::default();
        let job = jm.enqueue("task");
        let id = job.id.clone();
        jm.update_progress(&id, 150, Some("over".to_string()));
        let jobs = jm.list();
        let updated = jobs.iter().find(|j| j.id == id).unwrap();
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
        let updated = jobs.iter().find(|j| j.id == id).unwrap();
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
        let updated = jobs.iter().find(|j| j.id == id).unwrap();
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
        let updated = jobs.iter().find(|j| j.id == id).unwrap();
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
        let updated = jobs.iter().find(|j| j.id == id).unwrap();
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
        let paused = jobs.iter().find(|j| j.id == id).unwrap();
        assert_eq!(paused.status, JobStatus::Paused);
        assert_eq!(paused.detail.as_deref(), Some("waiting"));

        jm.resume(&id, None);
        let jobs = jm.list();
        let resumed = jobs.iter().find(|j| j.id == id).unwrap();
        assert_eq!(resumed.status, JobStatus::Running);
        assert_eq!(resumed.history.last().unwrap().phase, "resumed");
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
        let job = jm.list().into_iter().find(|j| j.id == id).unwrap();

        let encoded = JobManager::encode_persisted_detail(&job).unwrap().unwrap();
        let parsed = JobManager::parse_persisted_detail(Some(&encoded)).unwrap();

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
}
