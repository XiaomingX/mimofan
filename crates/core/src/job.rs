use std::collections::HashMap;

use anyhow::Result;
use mimofan_state::{JobStateRecord, JobStateStatus, StateStore};
use serde_json::{Value, json};
use uuid::Uuid;

pub(crate) const JOB_DETAIL_SCHEMA_VERSION: u8 = 1;
pub const DEFAULT_JOB_MAX_ATTEMPTS: u32 = 3;
pub const DEFAULT_JOB_BACKOFF_BASE_MS: u64 = 500;
pub const MAX_JOB_HISTORY_ENTRIES: usize = 64;

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
pub struct PersistedJobDetail {
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

    pub fn deterministic_backoff_ms(retry: &JobRetryMetadata) -> u64 {
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

    pub fn parse_persisted_detail(raw: Option<&str>) -> Option<PersistedJobDetail> {
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

    pub fn encode_persisted_detail(job: &JobRecord) -> Result<Option<String>> {
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

pub fn job_status_to_str(status: JobStatus) -> &'static str {
    match status {
        JobStatus::Queued => "queued",
        JobStatus::Running => "running",
        JobStatus::Paused => "paused",
        JobStatus::Completed => "completed",
        JobStatus::Failed => "failed",
        JobStatus::Cancelled => "cancelled",
    }
}

pub fn job_status_from_str(value: &str) -> Option<JobStatus> {
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

pub fn runtime_status_to_job_state(status: JobStatus) -> JobStateStatus {
    match status {
        JobStatus::Queued => JobStateStatus::Queued,
        JobStatus::Running => JobStateStatus::Running,
        JobStatus::Paused => JobStateStatus::Running,
        JobStatus::Completed => JobStateStatus::Completed,
        JobStatus::Failed => JobStateStatus::Failed,
        JobStatus::Cancelled => JobStateStatus::Cancelled,
    }
}

pub fn job_state_status_to_runtime(status: JobStateStatus) -> JobStatus {
    match status {
        JobStateStatus::Queued => JobStatus::Queued,
        JobStateStatus::Running => JobStatus::Running,
        JobStateStatus::Completed => JobStatus::Completed,
        JobStateStatus::Failed => JobStatus::Failed,
        JobStateStatus::Cancelled => JobStatus::Cancelled,
    }
}

pub fn json_optional_string(value: &Value) -> Option<String> {
    if value.is_null() {
        None
    } else {
        value.as_str().map(ToString::to_string)
    }
}

pub fn parse_retry_metadata(value: Option<&Value>) -> JobRetryMetadata {
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

pub fn parse_history_entry(value: &Value) -> Option<JobHistoryEntry> {
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

