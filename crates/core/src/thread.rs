use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Result;
use mimofan_protocol::{
    Thread, ThreadForkParams, ThreadGoal, ThreadGoalStatus, ThreadListParams, ThreadReadParams,
    ThreadResumeParams, ThreadSetNameParams, ThreadStatus,
};
pub use mimofan_protocol::{
    ThreadGoalClearParams, ThreadGoalGetParams, ThreadGoalProgressParams, ThreadGoalSetParams,
};
use mimofan_state::{
    SessionSource, StateStore, ThreadGoalRecord, ThreadGoalStatus as PersistedThreadGoalStatus,
    ThreadListFilters, ThreadMetadata, ThreadStatus as PersistedThreadStatus,
};
use serde_json::{Value, json};
use uuid::Uuid;

/// How a new thread's conversation history is initialized.
#[derive(Debug, Clone)]
pub enum InitialHistory {
    /// Start with an empty conversation.
    New,
    /// Forked from an existing thread with the given history items.
    Forked(Vec<Value>),
    /// Resumed from a persisted thread with its full history.
    Resumed {
        conversation_id: String,
        history: Vec<Value>,
        rollout_path: PathBuf,
    },
}

/// Result of spawning or resuming a thread.
#[derive(Debug, Clone)]
pub struct NewThread {
    /// The thread metadata.
    pub thread: Thread,
    /// Resolved model identifier.
    pub model: String,
    /// Provider that serves the model.
    pub model_provider: String,
    /// Working directory for the thread.
    pub cwd: PathBuf,
    /// Approval policy override, if any.
    pub approval_policy: Option<String>,
    /// Sandbox mode override, if any.
    pub sandbox: Option<String>,
}

/// Manages thread lifecycle: spawn, resume, fork, archive, and persistence.
pub struct ThreadManager {
    store: StateStore,
    running_threads: HashMap<String, Thread>,
    cli_version: String,
}

impl ThreadManager {
    /// Creates a new `ThreadManager` backed by the given state store.
    pub fn new(store: StateStore) -> Self {
        Self {
            store,
            running_threads: HashMap::new(),
            cli_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }

    /// Returns a reference to the underlying state store.
    pub fn state_store(&self) -> &StateStore {
        &self.store
    }

    /// Spawns a new thread with the given initial history and persists it.
    pub fn spawn_thread_with_history(
        &mut self,
        model_provider: String,
        cwd: PathBuf,
        initial_history: InitialHistory,
        persist_extended_history: bool,
    ) -> Result<NewThread> {
        let id = format!("thread-{}", Uuid::new_v4());
        let now = chrono::Utc::now().timestamp();
        let preview = preview_from_initial_history(&initial_history);
        let source = match initial_history {
            InitialHistory::New => SessionSource::Interactive,
            InitialHistory::Forked(_) => SessionSource::Fork,
            InitialHistory::Resumed { .. } => SessionSource::Resume,
        };
        let thread = Thread {
            id: id.clone(),
            preview,
            ephemeral: !persist_extended_history,
            model_provider: model_provider.clone(),
            created_at: now,
            updated_at: now,
            status: ThreadStatus::Running,
            path: None,
            cwd: cwd.clone(),
            cli_version: self.cli_version.clone(),
            source: match source {
                SessionSource::Interactive => mimofan_protocol::SessionSource::Interactive,
                SessionSource::Resume => mimofan_protocol::SessionSource::Resume,
                SessionSource::Fork => mimofan_protocol::SessionSource::Fork,
                SessionSource::Api => mimofan_protocol::SessionSource::Api,
                SessionSource::Unknown => mimofan_protocol::SessionSource::Unknown,
            },
            name: None,
        };
        self.persist_thread(&thread, None)?;
        match &initial_history {
            InitialHistory::Forked(items) => {
                for item in items {
                    self.store.append_message(
                        &thread.id,
                        "history",
                        &item.to_string(),
                        Some(item.clone()),
                    )?;
                }
            }
            InitialHistory::Resumed { history, .. } => {
                for item in history {
                    self.store.append_message(
                        &thread.id,
                        "history",
                        &item.to_string(),
                        Some(item.clone()),
                    )?;
                }
            }
            InitialHistory::New => {}
        }
        self.running_threads
            .insert(thread.id.clone(), thread.clone());
        Ok(NewThread {
            thread,
            model: "auto".to_string(),
            model_provider,
            cwd,
            approval_policy: None,
            sandbox: None,
        })
    }

    /// Resumes an existing thread, returning `None` if not found.
    pub fn resume_thread_with_history(
        &mut self,
        params: &ThreadResumeParams,
        fallback_cwd: &Path,
        model_provider: String,
    ) -> Result<Option<NewThread>> {
        if params.history.is_none()
            && let Some(thread) = self.running_threads.get(&params.thread_id).cloned()
        {
            return Ok(Some(NewThread {
                model: params.model.clone().unwrap_or_else(|| "auto".to_string()),
                model_provider: params.model_provider.clone().unwrap_or(model_provider),
                cwd: params.cwd.clone().unwrap_or_else(|| thread.cwd.clone()),
                approval_policy: params.approval_policy.clone(),
                sandbox: params.sandbox.clone(),
                thread,
            }));
        }

        let persisted = self.store.get_thread(&params.thread_id)?;
        let Some(metadata) = persisted else {
            return Ok(None);
        };
        let mut thread = to_protocol_thread(metadata);
        thread.status = ThreadStatus::Running;
        thread.updated_at = chrono::Utc::now().timestamp();
        thread.cwd = params
            .cwd
            .clone()
            .unwrap_or_else(|| fallback_cwd.to_path_buf());
        self.persist_thread(&thread, None)?;
        self.running_threads
            .insert(thread.id.clone(), thread.clone());
        if let Some(history) = params.history.as_ref() {
            for item in history {
                self.store.append_message(
                    &thread.id,
                    "history",
                    &item.to_string(),
                    Some(item.clone()),
                )?;
            }
        }

        Ok(Some(NewThread {
            model: params.model.clone().unwrap_or_else(|| "auto".to_string()),
            model_provider: params.model_provider.clone().unwrap_or(model_provider),
            cwd: thread.cwd.clone(),
            approval_policy: params.approval_policy.clone(),
            sandbox: params.sandbox.clone(),
            thread,
        }))
    }

    /// Forks an existing thread into a new one, inheriting the parent's provider.
    pub fn fork_thread(
        &mut self,
        params: &ThreadForkParams,
        fallback_cwd: &Path,
    ) -> Result<Option<NewThread>> {
        let parent = self.store.get_thread(&params.thread_id)?;
        let Some(parent) = parent else {
            return Ok(None);
        };
        let parent_thread = to_protocol_thread(parent);
        let new = self.spawn_thread_with_history(
            params
                .model_provider
                .clone()
                .unwrap_or_else(|| parent_thread.model_provider.clone()),
            params
                .cwd
                .clone()
                .unwrap_or_else(|| fallback_cwd.to_path_buf()),
            InitialHistory::Forked(vec![json!({
                "type": "fork",
                "from_thread_id": parent_thread.id
            })]),
            params.persist_extended_history,
        )?;
        Ok(Some(new))
    }

    /// Lists threads matching the given filter parameters.
    pub fn list_threads(&self, params: &ThreadListParams) -> Result<Vec<Thread>> {
        let list = self.store.list_threads(ThreadListFilters {
            include_archived: params.include_archived,
            limit: params.limit,
        })?;
        Ok(list.into_iter().map(to_protocol_thread).collect())
    }

    /// Reads a single thread by id, or `None` if not found.
    pub fn read_thread(&self, params: &ThreadReadParams) -> Result<Option<Thread>> {
        Ok(self
            .store
            .get_thread(&params.thread_id)?
            .map(to_protocol_thread))
    }

    /// Sets the display name for a thread, returning the updated thread or `None`.
    pub fn set_thread_name(&mut self, params: &ThreadSetNameParams) -> Result<Option<Thread>> {
        let Some(mut metadata) = self.store.get_thread(&params.thread_id)? else {
            return Ok(None);
        };
        metadata.name = Some(params.name.clone());
        metadata.updated_at = chrono::Utc::now().timestamp();
        self.store.upsert_thread(&metadata)?;
        let updated = to_protocol_thread(metadata);
        self.running_threads
            .insert(updated.id.clone(), updated.clone());
        Ok(Some(updated))
    }

    /// Sets or replaces the persisted goal for a thread.
    pub fn set_thread_goal(&mut self, params: &ThreadGoalSetParams) -> Result<Option<ThreadGoal>> {
        if self.store.get_thread(&params.thread_id)?.is_none() {
            return Ok(None);
        }
        let now = chrono::Utc::now().timestamp();
        let goal = ThreadGoalRecord {
            thread_id: params.thread_id.clone(),
            goal_id: format!("goal-{}", Uuid::new_v4()),
            objective: params.objective.clone(),
            status: PersistedThreadGoalStatus::Active,
            token_budget: params.token_budget,
            tokens_used: 0,
            time_used_seconds: 0,
            continuation_count: 0,
            created_at: now,
            updated_at: now,
        };
        self.store.upsert_thread_goal(&goal)?;
        Ok(Some(to_protocol_goal(goal)))
    }

    /// Reads the persisted goal for a thread.
    pub fn get_thread_goal(&self, params: &ThreadGoalGetParams) -> Result<Option<ThreadGoal>> {
        Ok(self
            .store
            .get_thread_goal(&params.thread_id)?
            .map(to_protocol_goal))
    }

    /// Accrues durable per-goal usage and/or a continuation pass for a thread.
    pub fn record_thread_goal_progress(
        &mut self,
        params: &ThreadGoalProgressParams,
    ) -> Result<Option<ThreadGoal>> {
        if self.store.get_thread(&params.thread_id)?.is_none() {
            return Ok(None);
        }

        let now = chrono::Utc::now().timestamp();
        let mut goal = if params.token_delta != 0 || params.time_delta_seconds != 0 {
            self.store.record_thread_goal_usage(
                &params.thread_id,
                params.token_delta,
                params.time_delta_seconds,
                now,
            )?
        } else {
            self.store.get_thread_goal(&params.thread_id)?
        };

        if params.record_continuation {
            goal = self
                .store
                .record_thread_goal_continuation(&params.thread_id, now)?;
        }

        Ok(goal.map(to_protocol_goal))
    }

    /// Clears the persisted goal for a thread, returning whether one existed.
    pub fn clear_thread_goal(&mut self, params: &ThreadGoalClearParams) -> Result<bool> {
        self.store.delete_thread_goal(&params.thread_id)
    }

    /// Archives a thread so it no longer appears in default listings.
    pub fn archive_thread(&mut self, thread_id: &str) -> Result<()> {
        self.store.mark_archived(thread_id)?;
        if let Some(thread) = self.running_threads.get_mut(thread_id) {
            thread.status = ThreadStatus::Archived;
        }
        Ok(())
    }

    /// Restores an archived thread to active status.
    pub fn unarchive_thread(&mut self, thread_id: &str) -> Result<()> {
        self.store.mark_unarchived(thread_id)?;
        Ok(())
    }

    /// Records a user message in a thread and updates its preview and timestamp.
    pub fn touch_message(&mut self, thread_id: &str, input: &str) -> Result<()> {
        let Some(mut metadata) = self.store.get_thread(thread_id)? else {
            return Ok(());
        };
        metadata.updated_at = chrono::Utc::now().timestamp();
        metadata.preview = truncate_preview(input);
        metadata.status = PersistedThreadStatus::Running;
        self.store.upsert_thread(&metadata)?;
        if let Some(thread) = self.running_threads.get_mut(thread_id) {
            thread.updated_at = metadata.updated_at;
            thread.preview = metadata.preview;
            thread.status = ThreadStatus::Running;
        }
        let message_id = self.store.append_message(thread_id, "user", input, None)?;
        self.store.save_checkpoint(
            thread_id,
            "latest",
            &json!({
                "reason": "thread_message",
                "message_id": message_id,
                "role": "user",
                "preview": truncate_preview(input),
                "updated_at": metadata.updated_at
            }),
        )?;
        Ok(())
    }

    pub(crate) fn persist_thread(
        &self,
        thread: &Thread,
        rollout_path: Option<PathBuf>,
    ) -> Result<()> {
        self.store.upsert_thread(&ThreadMetadata {
            id: thread.id.clone(),
            rollout_path,
            preview: thread.preview.clone(),
            ephemeral: thread.ephemeral,
            model_provider: thread.model_provider.clone(),
            created_at: thread.created_at,
            updated_at: thread.updated_at,
            status: to_persisted_status(&thread.status),
            path: thread.path.clone(),
            cwd: thread.cwd.clone(),
            cli_version: thread.cli_version.clone(),
            source: to_persisted_source(&thread.source),
            name: thread.name.clone(),
            sandbox_policy: None,
            approval_mode: None,
            archived: matches!(thread.status, ThreadStatus::Archived),
            archived_at: None,
            git_sha: None,
            git_branch: None,
            git_origin_url: None,
            memory_mode: None,
            current_leaf_id: None,
        })
    }
}

// ── Helper functions ──────────────────────────────────────────────────

pub(crate) fn truncate_preview(value: &str) -> String {
    value.chars().take(120).collect()
}

fn preview_from_initial_history(initial_history: &InitialHistory) -> String {
    match initial_history {
        InitialHistory::New => "New conversation".to_string(),
        InitialHistory::Forked(items) => truncate_preview(
            &items
                .first()
                .map(Value::to_string)
                .unwrap_or_else(|| "Forked conversation".to_string()),
        ),
        InitialHistory::Resumed { history, .. } => truncate_preview(
            &history
                .first()
                .map(Value::to_string)
                .unwrap_or_else(|| "Resumed conversation".to_string()),
        ),
    }
}

pub(crate) fn to_protocol_thread(thread: ThreadMetadata) -> Thread {
    Thread {
        id: thread.id,
        preview: thread.preview,
        ephemeral: thread.ephemeral,
        model_provider: thread.model_provider,
        created_at: thread.created_at,
        updated_at: thread.updated_at,
        status: match thread.status {
            PersistedThreadStatus::Running => ThreadStatus::Running,
            PersistedThreadStatus::Idle => ThreadStatus::Idle,
            PersistedThreadStatus::Completed => ThreadStatus::Completed,
            PersistedThreadStatus::Failed => ThreadStatus::Failed,
            PersistedThreadStatus::Paused => ThreadStatus::Paused,
            PersistedThreadStatus::Archived => ThreadStatus::Archived,
        },
        path: thread.path,
        cwd: thread.cwd,
        cli_version: thread.cli_version,
        source: match thread.source {
            SessionSource::Interactive => mimofan_protocol::SessionSource::Interactive,
            SessionSource::Resume => mimofan_protocol::SessionSource::Resume,
            SessionSource::Fork => mimofan_protocol::SessionSource::Fork,
            SessionSource::Api => mimofan_protocol::SessionSource::Api,
            SessionSource::Unknown => mimofan_protocol::SessionSource::Unknown,
        },
        name: thread.name,
    }
}

pub(crate) fn to_protocol_goal(goal: ThreadGoalRecord) -> ThreadGoal {
    ThreadGoal {
        thread_id: goal.thread_id,
        goal_id: goal.goal_id,
        objective: goal.objective,
        status: to_protocol_goal_status(goal.status),
        token_budget: goal.token_budget,
        tokens_used: goal.tokens_used,
        time_used_seconds: goal.time_used_seconds,
        continuation_count: goal.continuation_count,
        created_at: goal.created_at,
        updated_at: goal.updated_at,
    }
}

fn to_protocol_goal_status(status: PersistedThreadGoalStatus) -> ThreadGoalStatus {
    match status {
        PersistedThreadGoalStatus::Active => ThreadGoalStatus::Active,
        PersistedThreadGoalStatus::Paused => ThreadGoalStatus::Paused,
        PersistedThreadGoalStatus::Blocked => ThreadGoalStatus::Blocked,
        PersistedThreadGoalStatus::UsageLimited => ThreadGoalStatus::UsageLimited,
        PersistedThreadGoalStatus::BudgetLimited => ThreadGoalStatus::BudgetLimited,
        PersistedThreadGoalStatus::Complete => ThreadGoalStatus::Complete,
    }
}

pub(crate) fn to_persisted_status(status: &ThreadStatus) -> PersistedThreadStatus {
    match status {
        ThreadStatus::Running => PersistedThreadStatus::Running,
        ThreadStatus::Idle => PersistedThreadStatus::Idle,
        ThreadStatus::Completed => PersistedThreadStatus::Completed,
        ThreadStatus::Failed => PersistedThreadStatus::Failed,
        ThreadStatus::Paused => PersistedThreadStatus::Paused,
        ThreadStatus::Archived => PersistedThreadStatus::Archived,
    }
}

fn to_persisted_source(source: &mimofan_protocol::SessionSource) -> SessionSource {
    match source {
        mimofan_protocol::SessionSource::Interactive => SessionSource::Interactive,
        mimofan_protocol::SessionSource::Resume => SessionSource::Resume,
        mimofan_protocol::SessionSource::Fork => SessionSource::Fork,
        mimofan_protocol::SessionSource::Api => SessionSource::Api,
        mimofan_protocol::SessionSource::Unknown => SessionSource::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_preview_limits_to_120_chars() {
        let long = "a".repeat(200);
        let truncated = truncate_preview(&long);
        assert_eq!(truncated.len(), 120);
    }

    #[test]
    fn truncate_preview_preserves_short_strings() {
        let short = "hello";
        assert_eq!(truncate_preview(short), "hello");
    }

    #[test]
    fn preview_from_initial_history_new() {
        let preview = preview_from_initial_history(&InitialHistory::New);
        assert_eq!(preview, "New conversation");
    }

    #[test]
    fn preview_from_initial_history_forked() {
        let preview = preview_from_initial_history(&InitialHistory::Forked(vec![json!("hello")]));
        assert!(preview.contains("hello"));
    }

    #[test]
    fn preview_from_initial_history_resumed() {
        let preview = preview_from_initial_history(&InitialHistory::Resumed {
            conversation_id: "test".to_string(),
            history: vec![json!("world")],
            rollout_path: PathBuf::from("/tmp/test"),
        });
        assert!(preview.contains("world"));
    }
}
