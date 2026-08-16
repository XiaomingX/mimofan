//! Persistent state management for conversation threads, messages, and jobs.
//!
//! The [`StateStore`] is the primary entry point, backed by a SQLite database and an
//! append-only JSONL session index file. It provides CRUD operations for:
//!
//! - **Threads** — conversation metadata, archival, and session indexing.
//! - **Messages** — append-only message storage with tree-structured branching.
//! - **Checkpoints** — named state snapshots for restoring conversation progress.
//! - **Jobs** — background task tracking with status and progress.
//! - **Dynamic tools** — per-thread tool registrations.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Lifecycle status of a conversation thread.
///
/// Serialized as lowercase snake_case strings (e.g. `"running"`, `"archived"`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ThreadStatus {
    /// Thread is actively being worked on.
    Running,
    /// Thread exists but has no active work in progress.
    Idle,
    /// Thread has finished its task successfully.
    Completed,
    /// Thread encountered an unrecoverable error.
    Failed,
    /// Thread has been temporarily paused by the user.
    Paused,
    /// Thread has been archived and is hidden from default listings.
    Archived,
}

/// Indicates how a session was initiated.
///
/// Serialized as lowercase snake_case strings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionSource {
    /// Started by a user interacting with the CLI.
    Interactive,
    /// Resumed from a previously persisted session.
    Resume,
    /// Created by forking an existing conversation at a specific message.
    Fork,
    /// Initiated programmatically via the API.
    Api,
    /// Source is unknown or unspecified.
    Unknown,
}

/// Metadata for a persisted conversation thread.
///
/// Each thread represents a single conversation session and stores its
/// configuration, git context, and current status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadMetadata {
    /// Unique identifier for this thread.
    pub id: String,
    /// Optional filesystem path to the rollout (JSONL transcript) file.
    pub rollout_path: Option<PathBuf>,
    /// Short preview or summary of the thread content.
    pub preview: String,
    /// Whether this thread is ephemeral (not persisted long-term).
    pub ephemeral: bool,
    /// Identifier of the model provider used for this thread (e.g. `"openai"`).
    pub model_provider: String,
    /// Unix timestamp (seconds) when the thread was created.
    pub created_at: i64,
    /// Unix timestamp (seconds) of the most recent update to the thread.
    pub updated_at: i64,
    /// Current lifecycle status of the thread.
    pub status: ThreadStatus,
    /// Optional filesystem path associated with the thread working context.
    pub path: Option<PathBuf>,
    /// Working directory that was active when the thread was created.
    pub cwd: PathBuf,
    /// Version of the CLI that created this thread.
    pub cli_version: String,
    /// How this session was initiated.
    pub source: SessionSource,
    /// User-assigned display name for the thread.
    pub name: Option<String>,
    /// Serialized sandbox policy applied to this thread, if any.
    pub sandbox_policy: Option<String>,
    /// Approval mode configured for tool calls in this thread.
    pub approval_mode: Option<String>,
    /// Whether the thread has been archived.
    pub archived: bool,
    /// Unix timestamp (seconds) when the thread was archived, or `None` if not archived.
    pub archived_at: Option<i64>,
    /// Git commit SHA of the working tree when the thread was created.
    pub git_sha: Option<String>,
    /// Git branch checked out when the thread was created.
    pub git_branch: Option<String>,
    /// URL of the git remote origin, if available.
    pub git_origin_url: Option<String>,
    /// Memory mode configured for this thread (e.g. `"local"`, `"remote"`).
    pub memory_mode: Option<String>,
    /// ID of the current leaf message in the conversation tree.
    pub current_leaf_id: Option<i64>,
}

/// A dynamically registered tool associated with a thread.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicToolRecord {
    /// Ordinal position of this tool in the thread tool list.
    pub position: i64,
    /// Unique name identifying the tool.
    pub name: String,
    /// Human-readable description of what the tool does.
    pub description: Option<String>,
    /// JSON Schema describing the tool input parameters.
    pub input_schema: Value,
}

/// A single message entry in a conversation thread.
///
/// Messages form a tree structure via [`parent_entry_id`](Self::parent_entry_id),
/// enabling conversation branching and forking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageRecord {
    /// Auto-incremented unique identifier for this message.
    pub id: i64,
    /// ID of the thread this message belongs to.
    pub thread_id: String,
    /// Role of the message sender (e.g. `"user"`, `"assistant"`, `"system"`).
    pub role: String,
    /// Text content of the message.
    pub content: String,
    /// Optional structured item payload (tool calls, tool results, etc.).
    pub item: Option<Value>,
    /// Unix timestamp (seconds) when the message was created.
    pub created_at: i64,
    /// ID of the parent message, forming a tree structure. `None` for root messages.
    pub parent_entry_id: Option<i64>,
}

/// A named checkpoint capturing the state of a thread at a point in time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointRecord {
    /// ID of the thread this checkpoint belongs to.
    pub thread_id: String,
    /// Unique identifier for this checkpoint within its thread.
    pub checkpoint_id: String,
    /// Serialized state snapshot stored as a JSON value.
    pub state: Value,
    /// Unix timestamp (seconds) when the checkpoint was created or last updated.
    pub created_at: i64,
}

/// Status of a background job.
///
/// Serialized as lowercase snake_case strings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JobStateStatus {
    /// Job is waiting to be executed.
    Queued,
    /// Job is currently executing.
    Running,
    /// Job has finished successfully.
    Completed,
    /// Job has failed with an error.
    Failed,
    /// Job was cancelled before completion.
    Cancelled,
}

/// Persisted state of a background job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobStateRecord {
    /// Unique identifier for the job.
    pub id: String,
    /// Human-readable name describing the job.
    pub name: String,
    /// Current lifecycle status of the job.
    pub status: JobStateStatus,
    /// Completion progress as a percentage (0--100), if available.
    pub progress: Option<u8>,
    /// Optional detail message providing additional status information.
    pub detail: Option<String>,
    /// Unix timestamp (seconds) when the job was created.
    pub created_at: i64,
    /// Unix timestamp (seconds) of the most recent status update.
    pub updated_at: i64,
}

/// Persisted lifecycle status for a thread goal.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ThreadGoalStatus {
    /// Goal is active and should continue receiving work.
    Active,
    /// Goal is paused by the user.
    Paused,
    /// Goal is blocked and cannot make meaningful progress.
    Blocked,
    /// Goal stopped because account/service usage limits were reached.
    UsageLimited,
    /// Goal stopped because its explicit token budget was reached.
    BudgetLimited,
    /// Goal has been completed.
    Complete,
}

/// Persisted goal state attached to a thread.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ThreadGoalRecord {
    /// Thread this goal belongs to.
    pub thread_id: String,
    /// Stable identifier for this goal revision.
    pub goal_id: String,
    /// User-visible objective.
    pub objective: String,
    /// Current lifecycle status.
    pub status: ThreadGoalStatus,
    /// Optional token budget requested by the user.
    pub token_budget: Option<i64>,
    /// Tokens consumed while pursuing the goal.
    pub tokens_used: i64,
    /// Elapsed wall-clock work time in seconds.
    pub time_used_seconds: i64,
    /// Durable continuation passes dispatched for this objective.
    pub continuation_count: i64,
    /// Unix timestamp (seconds) when the goal was created.
    pub created_at: i64,
    /// Unix timestamp (seconds) when the goal was last updated.
    pub updated_at: i64,
}

/// Filters for listing conversation threads.
#[derive(Debug, Clone)]
pub struct ThreadListFilters {
    /// Whether to include archived threads in the results.
    pub include_archived: bool,
    /// Maximum number of threads to return. Defaults to 50.
    pub limit: Option<usize>,
}

impl Default for ThreadListFilters {
    fn default() -> Self {
        Self {
            include_archived: false,
            limit: Some(50),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SessionIndexEntry {
    thread_id: String,
    thread_name: Option<String>,
    updated_at: i64,
    rollout_path: Option<PathBuf>,
}

/// Persistent storage for conversation threads, messages, checkpoints, and jobs.
///
/// Backed by a SQLite database and an append-only JSONL session index file.
/// The database schema is automatically initialized and migrated on [`open`](Self::open).
#[derive(Debug, Clone)]
pub struct StateStore {
    db_path: PathBuf,
    session_index_path: PathBuf,
}

impl StateStore {
    /// Open (or create) a state store at the given database path.
    ///
    /// If `path` is `None`, the default location (`~/.mimofan/state.db`, with
    /// `~/.mimofan/state.db` as a legacy fallback) is used.
    /// The database schema is created automatically if it does not exist.
    pub fn open(path: Option<PathBuf>) -> Result<Self> {
        let db_path = path.unwrap_or_else(default_state_db_path);
        let session_index_path = db_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("session_index.jsonl");
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create state directory {}", parent.display())
            })?;
        }
        let store = Self {
            db_path,
            session_index_path,
        };
        store.init_schema()?;
        Ok(store)
    }

    /// Returns the filesystem path of the underlying SQLite database.
    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    pub fn conn(&self) -> Result<Connection> {
        let conn = Connection::open(&self.db_path)
            .with_context(|| format!("failed to open state db {}", self.db_path.display()))?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .with_context(|| {
                format!("failed to set busy timeout for {}", self.db_path.display())
            })?;
        let _ = conn.pragma_update(None, "journal_mode", "WAL");
        conn.pragma_update(None, "foreign_keys", "ON")
            .with_context(|| {
                format!(
                    "failed to enable foreign keys for {}",
                    self.db_path.display()
                )
            })?;
        Ok(conn)
    }

    fn init_schema(&self) -> Result<()> {
        let conn = self.conn()?;
        let mut user_version: u32 = conn.query_row("PRAGMA user_version;", [], |row| row.get(0))?;
        if user_version == 0 {
            conn.execute_batch(
                r#"
                BEGIN;
                CREATE TABLE IF NOT EXISTS threads (
                    id TEXT PRIMARY KEY,
                    rollout_path TEXT,
                    preview TEXT NOT NULL,
                    ephemeral INTEGER NOT NULL,
                    model_provider TEXT NOT NULL,
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL,
                    status TEXT NOT NULL,
                    path TEXT,
                    cwd TEXT NOT NULL,
                    cli_version TEXT NOT NULL,
                    source TEXT NOT NULL,
                    title TEXT,
                    sandbox_policy TEXT,
                    approval_mode TEXT,
                    archived INTEGER NOT NULL DEFAULT 0,
                    archived_at INTEGER,
                    git_sha TEXT,
                    git_branch TEXT,
                    git_origin_url TEXT,
                    memory_mode TEXT
                );
                CREATE INDEX IF NOT EXISTS idx_threads_updated_at ON threads(updated_at DESC);
                CREATE INDEX IF NOT EXISTS idx_threads_archived_at ON threads(archived_at DESC);
                CREATE INDEX IF NOT EXISTS idx_threads_archived_updated ON threads(archived, updated_at DESC);

                CREATE TABLE IF NOT EXISTS thread_dynamic_tools (
                    thread_id TEXT NOT NULL,
                    position INTEGER NOT NULL,
                    name TEXT NOT NULL,
                    description TEXT,
                    input_schema TEXT NOT NULL,
                    PRIMARY KEY (thread_id, position),
                    FOREIGN KEY(thread_id) REFERENCES threads(id) ON DELETE CASCADE
                );

                CREATE TABLE IF NOT EXISTS messages (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    thread_id TEXT NOT NULL,
                    role TEXT NOT NULL,
                    content TEXT NOT NULL,
                    item_json TEXT,
                    created_at INTEGER NOT NULL,
                    FOREIGN KEY(thread_id) REFERENCES threads(id) ON DELETE CASCADE
                );
                CREATE INDEX IF NOT EXISTS idx_messages_thread_created_at ON messages(thread_id, created_at ASC);

                CREATE TABLE IF NOT EXISTS checkpoints (
                    thread_id TEXT NOT NULL,
                    checkpoint_id TEXT NOT NULL,
                    state_json TEXT NOT NULL,
                    created_at INTEGER NOT NULL,
                    PRIMARY KEY(thread_id, checkpoint_id),
                    FOREIGN KEY(thread_id) REFERENCES threads(id) ON DELETE CASCADE
                );
                CREATE INDEX IF NOT EXISTS idx_checkpoints_thread_created_at ON checkpoints(thread_id, created_at DESC);

                CREATE TABLE IF NOT EXISTS jobs (
                    id TEXT PRIMARY KEY,
                    name TEXT NOT NULL,
                    status TEXT NOT NULL,
                    progress INTEGER,
                    detail TEXT,
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_jobs_updated_at ON jobs(updated_at DESC);

                -- Add parent_entry_id column, and set to last message before current message
                ALTER TABLE messages ADD COLUMN parent_entry_id INTEGER NULL;
                UPDATE messages
                    SET parent_entry_id = (
                        SELECT m2.id
                        FROM messages m2
                        WHERE m2.thread_id = messages.thread_id
                            AND (
                                m2.created_at < messages.created_at
                                OR (
                                    m2.created_at = messages.created_at
                                    AND m2.id < messages.id
                                )
                            )
                        ORDER BY m2.created_at DESC, m2.id DESC
                        LIMIT 1
                    );
                CREATE INDEX idx_messages_parent_entry_id ON messages(parent_entry_id);

                -- Add current_leaf_id column, and set to last message in thread
                ALTER TABLE threads ADD COLUMN current_leaf_id INTEGER NULL;
                UPDATE threads
                    SET current_leaf_id = (
                        SELECT m.id
                        FROM messages m
                        WHERE m.thread_id = threads.id
                        ORDER BY m.id DESC
                        LIMIT 1
                    );

                PRAGMA user_version = 1;
                COMMIT;
                "#,
            )
            .context("failed to initialize thread schema")?;
            user_version = 1;
        }
        if user_version < 2 {
            conn.execute_batch(
                r#"
                BEGIN;
                CREATE TABLE IF NOT EXISTS workflow_runs (
                    id TEXT PRIMARY KEY,
                    workflow_id TEXT NOT NULL,
                    goal TEXT NOT NULL,
                    status TEXT NOT NULL,
                    input_hash TEXT,
                    started_at INTEGER NOT NULL,
                    completed_at INTEGER,
                    metadata_json TEXT NOT NULL DEFAULT '{}'
                );
                CREATE INDEX IF NOT EXISTS idx_workflow_runs_status_started_at
                    ON workflow_runs(status, started_at DESC);
                CREATE INDEX IF NOT EXISTS idx_workflow_runs_workflow_started_at
                    ON workflow_runs(workflow_id, started_at DESC);

                CREATE TABLE IF NOT EXISTS branch_runs (
                    id TEXT PRIMARY KEY,
                    workflow_run_id TEXT NOT NULL,
                    branch_id TEXT NOT NULL,
                    node_id TEXT NOT NULL,
                    status TEXT NOT NULL,
                    started_at INTEGER NOT NULL,
                    completed_at INTEGER,
                    result_json TEXT NOT NULL DEFAULT '{}',
                    FOREIGN KEY(workflow_run_id) REFERENCES workflow_runs(id) ON DELETE CASCADE
                );
                CREATE INDEX IF NOT EXISTS idx_branch_runs_workflow_run_id
                    ON branch_runs(workflow_run_id);
                CREATE INDEX IF NOT EXISTS idx_branch_runs_branch_id
                    ON branch_runs(branch_id);

                CREATE TABLE IF NOT EXISTS leaf_runs (
                    id TEXT PRIMARY KEY,
                    workflow_run_id TEXT NOT NULL,
                    branch_run_id TEXT,
                    leaf_id TEXT NOT NULL,
                    task_id TEXT NOT NULL,
                    input_hash TEXT,
                    status TEXT NOT NULL,
                    output_json TEXT NOT NULL DEFAULT '{}',
                    artifacts_json TEXT NOT NULL DEFAULT '[]',
                    started_at INTEGER NOT NULL,
                    completed_at INTEGER,
                    FOREIGN KEY(workflow_run_id) REFERENCES workflow_runs(id) ON DELETE CASCADE,
                    FOREIGN KEY(branch_run_id) REFERENCES branch_runs(id) ON DELETE SET NULL
                );
                CREATE INDEX IF NOT EXISTS idx_leaf_runs_workflow_run_id
                    ON leaf_runs(workflow_run_id);
                CREATE INDEX IF NOT EXISTS idx_leaf_runs_replay_lookup
                    ON leaf_runs(workflow_run_id, leaf_id, input_hash);

                CREATE TABLE IF NOT EXISTS control_node_runs (
                    id TEXT PRIMARY KEY,
                    workflow_run_id TEXT NOT NULL,
                    node_id TEXT NOT NULL,
                    kind TEXT NOT NULL,
                    status TEXT NOT NULL,
                    selected_children_json TEXT NOT NULL DEFAULT '[]',
                    result_json TEXT NOT NULL DEFAULT '{}',
                    started_at INTEGER NOT NULL,
                    completed_at INTEGER,
                    FOREIGN KEY(workflow_run_id) REFERENCES workflow_runs(id) ON DELETE CASCADE
                );
                CREATE INDEX IF NOT EXISTS idx_control_node_runs_workflow_run_id
                    ON control_node_runs(workflow_run_id);
                CREATE INDEX IF NOT EXISTS idx_control_node_runs_node_id
                    ON control_node_runs(node_id);

                CREATE TABLE IF NOT EXISTS teacher_candidates (
                    id TEXT PRIMARY KEY,
                    workflow_run_id TEXT NOT NULL,
                    control_node_run_id TEXT NOT NULL,
                    candidate_id TEXT NOT NULL,
                    branch_run_id TEXT,
                    score REAL,
                    passed INTEGER,
                    rationale_json TEXT NOT NULL DEFAULT '{}',
                    created_at INTEGER NOT NULL,
                    FOREIGN KEY(workflow_run_id) REFERENCES workflow_runs(id) ON DELETE CASCADE,
                    FOREIGN KEY(control_node_run_id) REFERENCES control_node_runs(id) ON DELETE CASCADE,
                    FOREIGN KEY(branch_run_id) REFERENCES branch_runs(id) ON DELETE SET NULL
                );
                CREATE INDEX IF NOT EXISTS idx_teacher_candidates_workflow_run_id
                    ON teacher_candidates(workflow_run_id);
                CREATE INDEX IF NOT EXISTS idx_teacher_candidates_control_node_run_id
                    ON teacher_candidates(control_node_run_id);

                PRAGMA user_version = 2;
                COMMIT;
                "#,
            )
            .context("failed to initialize workflow trace schema")?;
            user_version = 2;
        }
        if user_version < 3 {
            conn.execute_batch(
                r#"
                BEGIN;
                CREATE TABLE IF NOT EXISTS thread_goals (
                    thread_id TEXT PRIMARY KEY NOT NULL,
                    goal_id TEXT NOT NULL,
                    objective TEXT NOT NULL,
                    status TEXT NOT NULL CHECK(status IN (
                        'active',
                        'paused',
                        'blocked',
                        'usage_limited',
                        'budget_limited',
                        'complete'
                    )),
                    token_budget INTEGER,
                    tokens_used INTEGER NOT NULL DEFAULT 0,
                    time_used_seconds INTEGER NOT NULL DEFAULT 0,
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL,
                    FOREIGN KEY(thread_id) REFERENCES threads(id) ON DELETE CASCADE
                );

                PRAGMA user_version = 3;
                COMMIT;
                "#,
            )
            .context("failed to initialize thread goal schema")?;
            user_version = 3;
        }
        if user_version < 4 {
            conn.execute_batch(
                r#"
                BEGIN;
                ALTER TABLE thread_goals
                    ADD COLUMN continuation_count INTEGER NOT NULL DEFAULT 0;

                PRAGMA user_version = 4;
                COMMIT;
                "#,
            )
            .context("failed to initialize thread goal continuation schema")?;
        }
        Ok(())
    }

    /// Insert or update thread metadata.
    ///
    /// This does **not** update `current_leaf_id`; use [`append_message`](Self::append_message)
    /// or [`set_current_leaf_id`](Self::set_current_leaf_id) for that.
    pub fn upsert_thread(&self, thread: &ThreadMetadata) -> Result<()> {
        let conn = self.conn()?;
        // Auto-assign a default JSONL transcript path when the caller didn't
        // supply one, so every persisted thread gets a structured copy. This
        // is additive: existing threads already stored with `None` keep their
        // behavior (their `append_message` simply skips mirroring).
        let effective_rollout_path = thread
            .rollout_path
            .clone()
            .or_else(|| self.default_rollout_path(&thread.id));
        conn.execute(
            r#"
            INSERT INTO threads (
                id, rollout_path, preview, ephemeral, model_provider, created_at, updated_at, status, path, cwd,
                cli_version, source, title, sandbox_policy, approval_mode, archived, archived_at,
                git_sha, git_branch, git_origin_url, memory_mode
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                ?11, ?12, ?13, ?14, ?15, ?16, ?17,
                ?18, ?19, ?20, ?21
            )
            ON CONFLICT(id) DO UPDATE SET
                rollout_path=excluded.rollout_path,
                preview=excluded.preview,
                ephemeral=excluded.ephemeral,
                model_provider=excluded.model_provider,
                created_at=excluded.created_at,
                updated_at=excluded.updated_at,
                status=excluded.status,
                path=excluded.path,
                cwd=excluded.cwd,
                cli_version=excluded.cli_version,
                source=excluded.source,
                title=excluded.title,
                sandbox_policy=excluded.sandbox_policy,
                approval_mode=excluded.approval_mode,
                archived=excluded.archived,
                archived_at=excluded.archived_at,
                git_sha=excluded.git_sha,
                git_branch=excluded.git_branch,
                git_origin_url=excluded.git_origin_url,
                memory_mode=excluded.memory_mode
            "#,
            params![
                thread.id,
                path_to_opt_string(effective_rollout_path.as_deref()),
                thread.preview,
                bool_to_i64(thread.ephemeral),
                thread.model_provider,
                thread.created_at,
                thread.updated_at,
                thread_status_to_str(&thread.status),
                path_to_opt_string(thread.path.as_deref()),
                thread.cwd.display().to_string(),
                thread.cli_version,
                session_source_to_str(&thread.source),
                thread.name,
                thread.sandbox_policy,
                thread.approval_mode,
                bool_to_i64(thread.archived),
                thread.archived_at,
                thread.git_sha,
                thread.git_branch,
                thread.git_origin_url,
                thread.memory_mode,
            ],
        )
        .context("failed to upsert thread metadata")?;

        self.append_thread_name(
            &thread.id,
            thread.name.clone(),
            thread.updated_at,
            effective_rollout_path.clone(),
        )?;
        Ok(())
    }

    /// Retrieve a single thread by its ID.
    ///
    /// Returns `None` if no thread with the given ID exists.
    pub fn get_thread(&self, id: &str) -> Result<Option<ThreadMetadata>> {
        let conn = self.conn()?;
        conn.query_row(
            r#"
            SELECT id, rollout_path, preview, ephemeral, model_provider, created_at, updated_at, status, path, cwd,
                   cli_version, source, title, sandbox_policy, approval_mode, archived, archived_at,
                   git_sha, git_branch, git_origin_url, memory_mode, current_leaf_id
            FROM threads
            WHERE id = ?1
            "#,
            params![id],
            row_to_thread,
        )
        .optional()
        .context("failed to read thread")
    }

    /// List threads ordered by most recently updated.
    ///
    /// Use [`ThreadListFilters`] to control whether archived threads are included
    /// and the maximum number of results returned.
    pub fn list_threads(&self, filters: ThreadListFilters) -> Result<Vec<ThreadMetadata>> {
        let conn = self.conn()?;
        let sql = if filters.include_archived {
            "SELECT id, rollout_path, preview, ephemeral, model_provider, created_at, updated_at, status, path, cwd, cli_version, source, title, sandbox_policy, approval_mode, archived, archived_at, git_sha, git_branch, git_origin_url, memory_mode, current_leaf_id FROM threads ORDER BY updated_at DESC LIMIT ?1"
        } else {
            "SELECT id, rollout_path, preview, ephemeral, model_provider, created_at, updated_at, status, path, cwd, cli_version, source, title, sandbox_policy, approval_mode, archived, archived_at, git_sha, git_branch, git_origin_url, memory_mode, current_leaf_id FROM threads WHERE archived = 0 ORDER BY updated_at DESC LIMIT ?1"
        };

        let mut stmt = conn.prepare(sql).context("failed to prepare list query")?;
        let limit = i64::try_from(filters.limit.unwrap_or(50)).unwrap_or(50);
        let mut rows = stmt
            .query(params![limit])
            .context("failed to query threads")?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().context("failed to iterate thread rows")? {
            out.push(row_to_thread(row)?);
        }
        Ok(out)
    }

    /// Archive a thread, setting its status to [`ThreadStatus::Archived`] and
    /// recording the current timestamp.
    pub fn mark_archived(&self, id: &str) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE threads SET archived = 1, archived_at = ?2, status = ?3 WHERE id = ?1",
            params![
                id,
                Utc::now().timestamp(),
                thread_status_to_str(&ThreadStatus::Archived)
            ],
        )
        .context("failed to archive thread")?;
        Ok(())
    }

    /// Unarchive a thread, removing the archived flag and clearing `archived_at`.
    pub fn mark_unarchived(&self, id: &str) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE threads SET archived = 0, archived_at = NULL WHERE id = ?1",
            params![id],
        )
        .context("failed to unarchive thread")?;
        Ok(())
    }

    /// Permanently delete a thread and all of its associated data
    /// (messages, checkpoints, dynamic tools) via cascading foreign keys.
    pub fn delete_thread(&self, id: &str) -> Result<()> {
        let conn = self.conn()?;
        conn.execute("DELETE FROM threads WHERE id = ?1", params![id])
            .context("failed to delete thread")?;
        Ok(())
    }

    /// Set the memory mode for a thread.
    ///
    /// Pass `None` to clear the memory mode.
    pub fn set_thread_memory_mode(&self, id: &str, mode: Option<&str>) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE threads SET memory_mode = ?2 WHERE id = ?1",
            params![id, mode],
        )
        .context("failed to update thread memory mode")?;
        Ok(())
    }

    /// Get the memory mode configured for a thread.
    ///
    /// Returns `None` if the thread does not exist or has no memory mode set.
    pub fn get_thread_memory_mode(&self, id: &str) -> Result<Option<String>> {
        let conn = self.conn()?;
        conn.query_row(
            "SELECT memory_mode FROM threads WHERE id = ?1",
            params![id],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
        .context("failed to read thread memory mode")
        .map(Option::flatten)
    }

    /// Insert or replace the persisted goal for a thread.
    pub fn upsert_thread_goal(&self, goal: &ThreadGoalRecord) -> Result<()> {
        let conn = self.conn()?;
        let exists: Option<i64> = conn
            .query_row(
                "SELECT 1 FROM threads WHERE id = ?1",
                params![goal.thread_id],
                |row| row.get(0),
            )
            .optional()
            .context("failed to verify thread before saving goal")?;
        if exists.is_none() {
            anyhow::bail!("thread {} not found", goal.thread_id);
        }

        conn.execute(
            r#"
            INSERT INTO thread_goals (
                thread_id, goal_id, objective, status, token_budget, tokens_used,
                time_used_seconds, continuation_count, created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            ON CONFLICT(thread_id) DO UPDATE SET
                goal_id=excluded.goal_id,
                objective=excluded.objective,
                status=excluded.status,
                token_budget=excluded.token_budget,
                tokens_used=excluded.tokens_used,
                time_used_seconds=excluded.time_used_seconds,
                continuation_count=excluded.continuation_count,
                created_at=excluded.created_at,
                updated_at=excluded.updated_at
            "#,
            params![
                goal.thread_id,
                goal.goal_id,
                goal.objective,
                thread_goal_status_to_str(&goal.status),
                goal.token_budget,
                goal.tokens_used,
                goal.time_used_seconds,
                goal.continuation_count,
                goal.created_at,
                goal.updated_at,
            ],
        )
        .context("failed to upsert thread goal")?;
        Ok(())
    }

    /// Accrue additional token and wall-clock usage onto a thread's persisted goal.
    ///
    /// This is the durable, additive accounting path for the persistent goal loop: it
    /// increments `tokens_used` and `time_used_seconds` in a single atomic SQL `UPDATE`
    /// (`col = col + ?`) so concurrent accruals do not race a read-modify-write. The
    /// goal's `updated_at` is advanced to the larger of its current value and `now`,
    /// keeping the timestamp monotonic even if a stale `now` is supplied.
    ///
    /// `token_delta` and `time_delta_seconds` are added on the database side; callers
    /// should pass non-negative deltas (negative values are accepted and will decrement,
    /// which is intentionally left to the caller's discretion).
    ///
    /// Returns the updated [`ThreadGoalRecord`], or `Ok(None)` if the thread has no
    /// persisted goal. Unlike [`upsert_thread_goal`](Self::upsert_thread_goal) this never
    /// creates a goal row; it only accumulates onto an existing one.
    pub fn record_thread_goal_usage(
        &self,
        thread_id: &str,
        token_delta: i64,
        time_delta_seconds: i64,
        now: i64,
    ) -> Result<Option<ThreadGoalRecord>> {
        let conn = self.conn()?;
        let changed = conn
            .execute(
                r#"
                UPDATE thread_goals
                SET tokens_used = tokens_used + ?2,
                    time_used_seconds = time_used_seconds + ?3,
                    updated_at = MAX(updated_at, ?4)
                WHERE thread_id = ?1
                "#,
                params![thread_id, token_delta, time_delta_seconds, now],
            )
            .context("failed to record thread goal usage")?;
        if changed == 0 {
            return Ok(None);
        }
        self.get_thread_goal(thread_id)
    }

    /// Increment the durable cross-turn continuation counter for a thread goal.
    ///
    /// The older TUI continuation guard is scoped to one engine turn. This
    /// counter is intentionally persisted so a resumed goal loop can feed
    /// `goal_loop::decide_continuation` with the true cross-turn count.
    pub fn record_thread_goal_continuation(
        &self,
        thread_id: &str,
        now: i64,
    ) -> Result<Option<ThreadGoalRecord>> {
        let conn = self.conn()?;
        let changed = conn
            .execute(
                r#"
                UPDATE thread_goals
                SET continuation_count = continuation_count + 1,
                    updated_at = MAX(updated_at, ?2)
                WHERE thread_id = ?1
                "#,
                params![thread_id, now],
            )
            .context("failed to record thread goal continuation")?;
        if changed == 0 {
            return Ok(None);
        }
        self.get_thread_goal(thread_id)
    }

    /// Retrieve the persisted goal for a thread.
    pub fn get_thread_goal(&self, thread_id: &str) -> Result<Option<ThreadGoalRecord>> {
        let conn = self.conn()?;
        conn.query_row(
            r#"
            SELECT thread_id, goal_id, objective, status, token_budget, tokens_used,
                   time_used_seconds, continuation_count, created_at, updated_at
            FROM thread_goals
            WHERE thread_id = ?1
            "#,
            params![thread_id],
            row_to_thread_goal,
        )
        .optional()
        .context("failed to read thread goal")
    }

    /// Delete the persisted goal for a thread.
    pub fn delete_thread_goal(&self, thread_id: &str) -> Result<bool> {
        let conn = self.conn()?;
        let changed = conn
            .execute(
                "DELETE FROM thread_goals WHERE thread_id = ?1",
                params![thread_id],
            )
            .context("failed to delete thread goal")?;
        Ok(changed > 0)
    }

    /// List all leaf messages in a thread.
    ///
    /// A leaf message is one that has no other message referencing it as a parent.
    /// In a branching conversation tree, there may be multiple leaf messages.
    pub fn list_leaf_messages(&self, thread_id: &str) -> Result<Vec<MessageRecord>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                r#"
                SELECT m1.id, m1.thread_id, m1.role, m1.content, m1.item_json, m1.created_at, m1.parent_entry_id
                FROM messages m1
                LEFT JOIN messages m2 ON m1.id = m2.parent_entry_id
                WHERE m1.thread_id = ?1 AND m2.id IS NULL
                "#,
            )
            .context("failed to prepare message listing query")?;
        let mut rows = stmt
            .query(params![thread_id])
            .with_context(|| format!("failed to list leaf messages for thread {thread_id}"))?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().context("failed to iterate message rows")? {
            let item_json: Option<String> = row.get(4).context("failed to read item json")?;
            let item = item_json
                .as_deref()
                .map(serde_json::from_str)
                .transpose()
                .with_context(|| {
                    format!("failed to parse message item json in thread {thread_id}")
                })?;
            out.push(MessageRecord {
                id: row.get(0).context("failed to read message id")?,
                thread_id: row.get(1).context("failed to read message thread id")?,
                role: row.get(2).context("failed to read message role")?,
                content: row.get(3).context("failed to read message content")?,
                item,
                created_at: row.get(5).context("failed to read message timestamp")?,
                parent_entry_id: row.get(6).context("failed to read parent entry id")?,
            });
        }
        Ok(out)
    }

    /// Update the current leaf message pointer for a thread.
    ///
    /// This controls which branch of the conversation tree is considered active
    /// when listing messages via [`list_messages`](Self::list_messages).
    pub fn set_current_leaf_id(&self, thread_id: &str, current_leaf_id: &str) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE threads SET current_leaf_id = ?1 WHERE id = ?2",
            params![current_leaf_id, thread_id],
        )
        .context("failed to update thread current leaf id")?;
        Ok(())
    }

    /// Replace the dynamic tools for a thread.
    ///
    /// All existing dynamic tools for the thread are deleted and replaced with the
    /// provided list. The operation is performed within a transaction.
    pub fn persist_dynamic_tools(
        &self,
        thread_id: &str,
        tools: &[DynamicToolRecord],
    ) -> Result<()> {
        let mut conn = self.conn()?;
        let tx = conn
            .transaction()
            .context("failed to begin dynamic tools transaction")?;
        tx.execute(
            "DELETE FROM thread_dynamic_tools WHERE thread_id = ?1",
            params![thread_id],
        )
        .context("failed to clear dynamic tools")?;
        for tool in tools {
            tx.execute(
                "INSERT INTO thread_dynamic_tools(thread_id, position, name, description, input_schema) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    thread_id,
                    tool.position,
                    tool.name,
                    tool.description,
                    tool.input_schema.to_string()
                ],
            )
            .with_context(|| format!("failed to persist dynamic tool {}", tool.name))?;
        }
        tx.commit().context("failed to commit dynamic tools")?;
        Ok(())
    }

    /// Retrieve all dynamic tools registered for a thread, ordered by position.
    pub fn get_dynamic_tools(&self, thread_id: &str) -> Result<Vec<DynamicToolRecord>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT position, name, description, input_schema FROM thread_dynamic_tools WHERE thread_id = ?1 ORDER BY position ASC",
            )
            .context("failed to prepare get dynamic tools query")?;
        let mut rows = stmt
            .query(params![thread_id])
            .context("failed to query dynamic tools")?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().context("failed to iterate dynamic tools")? {
            let input_schema_raw: String =
                row.get(3).context("failed to read tool input schema")?;
            let input_schema: Value =
                serde_json::from_str(&input_schema_raw).with_context(|| {
                    format!("failed to parse input schema for dynamic tool in thread {thread_id}")
                })?;
            out.push(DynamicToolRecord {
                position: row.get(0).context("failed to read tool position")?,
                name: row.get(1).context("failed to read tool name")?,
                description: row.get(2).context("failed to read tool description")?,
                input_schema,
            });
        }
        Ok(out)
    }

    /// Append a new message to a thread.
    ///
    /// The message is linked to the thread's current leaf as its parent, and the
    /// thread's `current_leaf_id` is updated to the new message. Returns the ID
    /// of the newly created message.
    pub fn append_message(
        &self,
        thread_id: &str,
        role: &str,
        content: &str,
        item: Option<Value>,
    ) -> Result<i64> {
        let mut conn = self.conn()?;
        let created_at = Utc::now().timestamp();
        let item_json = item
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .context("failed to serialize message item payload")?;

        let tx = conn
            .transaction()
            .context("failed to begin append message transaction")?;

        let current_leaf_id: Option<i64> = tx
            .query_row(
                "SELECT current_leaf_id FROM threads WHERE id = ?1",
                params![thread_id],
                |row| row.get(0),
            )
            .with_context(|| {
                format!("failed to query thread current leaf id for thread {thread_id}")
            })?;

        let next_leaf_id: i64 = tx.query_row(
            r#"
                INSERT INTO messages(thread_id, role, content, item_json, created_at, parent_entry_id)
                SELECT ?1, ?2, ?3, ?4, ?5, ?6
                RETURNING id
            "#, params![thread_id, role, content, item_json, created_at, current_leaf_id], |row| row.get(0)
        ).with_context(|| format!("failed to append message for thread {thread_id}"))?;

        tx.execute(
            r#"
            UPDATE threads
            SET current_leaf_id = ?1
            WHERE id = ?2;
            "#,
            params![next_leaf_id, thread_id],
        )
        .with_context(|| {
            format!("failed to update thread current leaf id for thread {thread_id}")
        })?;

        tx.commit()
            .context("failed to commit append message transaction")?;

        // Mirror the message into the thread's JSONL transcript (a structured
        // file copy of the conversation, independent of the SQLite row model).
        // Pure additive: the SQLite `messages` table remains the source of
        // truth for in-app loading, so this never affects replay/read paths.
        self.persist_transcript_line(thread_id, role, content, item_json.as_deref(), created_at)
            .ok(); // best-effort: a transcript write failure must not fail the turn

        Ok(next_leaf_id)
    }

    /// Append one message as a single JSON line to the thread's rollout
    /// (JSONL transcript) file, if the thread has a `rollout_path` set.
    ///
    /// This is additive and best-effort: transcript I/O errors are swallowed
    /// by the caller (`persist_transcript_line(...).ok()`) so a disk/permission
    /// issue can never abort a model turn. The `item` payload (when present)
    /// carries the full structured content (tool calls, content blocks), while
    /// `content` is the flat human-readable fallback.
    fn persist_transcript_line(
        &self,
        thread_id: &str,
        role: &str,
        content: &str,
        item_json: Option<&str>,
        created_at: i64,
    ) -> Result<()> {
        let rollout_path = match self.find_rollout_path_by_id(thread_id)? {
            Some(path) => path,
            None => return Ok(()), // no transcript configured for this thread
        };
        if let Some(parent) = rollout_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create rollout dir {}", parent.display()))?;
        }
        let line = serde_json::json!({
            "ts": created_at,
            "thread_id": thread_id,
            "role": role,
            "content": content,
            "item": item_json.and_then(|s| serde_json::from_str::<Value>(s).ok()),
        });
        let encoded =
            serde_json::to_string(&line).context("failed to serialize transcript line")?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&rollout_path)
            .with_context(|| format!("failed to open rollout {}", rollout_path.display()))?;
        writeln!(file, "{encoded}").context("failed to append transcript line")?;
        Ok(())
    }

    /// Read a thread's JSONL transcript back into structured records.
    ///
    /// Returns `None` when the thread has no `rollout_path` (e.g. threads
    /// created before transcript mirroring existed). Each line is parsed as a
    /// JSON object; malformed lines are skipped so a single corrupt row can't
    /// sink the whole replay. This is an additive read path alongside the
    /// SQLite `list_messages` source of truth.
    pub fn read_transcript(&self, thread_id: &str) -> Result<Option<Vec<Value>>> {
        let Some(path) = self.find_rollout_path_by_id(thread_id)? else {
            return Ok(None);
        };
        if !path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(&path)
            .with_context(|| format!("failed to read rollout {}", path.display()))?;
        let mut lines = Vec::new();
        for line in content.lines().filter(|l| !l.trim().is_empty()) {
            match serde_json::from_str::<Value>(line) {
                Ok(value) => lines.push(value),
                Err(err) => {
                    tracing::warn!(
                        ?err,
                        "skipping malformed transcript line in {}",
                        path.display()
                    );
                }
            }
        }
        Ok(Some(lines))
    }

    /// List messages in the current conversation branch, walking backwards from
    /// the thread's `current_leaf_id`.
    ///
    /// Messages are returned in chronological order (oldest first). The `limit`
    /// parameter caps how many ancestor messages are traversed; it defaults to 500.
    pub fn list_messages(
        &self,
        thread_id: &str,
        limit: Option<usize>,
    ) -> Result<Vec<MessageRecord>> {
        let conn = self.conn()?;
        let limit = i64::try_from(limit.unwrap_or(500)).unwrap_or(500);
        let mut stmt = conn
            .prepare(
                r#"
                WITH RECURSIVE
                    leaf_id AS (
                        SELECT current_leaf_id FROM threads WHERE id = ?1
                    ),
                    ancestors AS (
                        SELECT id, thread_id, role, content, item_json, created_at, parent_entry_id, 0 AS depth
                        FROM messages
                        WHERE id = (SELECT current_leaf_id FROM leaf_id)

                        UNION ALL

                        SELECT m.id, m.thread_id, m.role, m.content, m.item_json, m.created_at, m.parent_entry_id, a.depth + 1
                        FROM messages m
                        JOIN ancestors a ON m.id = a.parent_entry_id
                        WHERE a.depth < ?2
                    )
                    SELECT id, thread_id, role, content, item_json, created_at, parent_entry_id FROM ancestors
                    ORDER BY depth DESC
                "#
            )
            .context("failed to prepare message listing query")?;
        let mut rows = stmt
            .query(params![thread_id, limit - 1])
            .with_context(|| format!("failed to list messages for thread {thread_id}"))?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().context("failed to iterate message rows")? {
            let item_json: Option<String> = row.get(4).context("failed to read item json")?;
            let item = item_json
                .as_deref()
                .map(serde_json::from_str)
                .transpose()
                .with_context(|| {
                    format!("failed to parse message item json in thread {thread_id}")
                })?;
            out.push(MessageRecord {
                id: row.get(0).context("failed to read message id")?,
                thread_id: row.get(1).context("failed to read message thread id")?,
                role: row.get(2).context("failed to read message role")?,
                content: row.get(3).context("failed to read message content")?,
                item,
                created_at: row.get(5).context("failed to read message timestamp")?,
                parent_entry_id: row.get(6).context("failed to read parent entry id")?,
            });
        }
        Ok(out)
    }

    /// Fork the conversation at a specific message.
    ///
    /// Creates a new message whose parent is `message_id` and updates the thread's
    /// `current_leaf_id` to the new message. Returns the ID of the new message.
    /// This enables branching conversations from any point in the history.
    pub fn fork_at_message(
        &self,
        message_id: &str,
        role: &str,
        content: &str,
        item: Option<Value>,
    ) -> Result<i64> {
        let mut conn = self.conn()?;
        let created_at = Utc::now().timestamp();
        let item_json = item
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .context("failed to serialize message item payload")?;

        let tx = conn
            .transaction()
            .context("failed to begin fork message transaction")?;

        let thread_id: String = tx
            .query_row(
                "SELECT thread_id FROM messages WHERE id = ?1",
                params![message_id],
                |row| row.get(0),
            )
            .with_context(|| format!("failed to query thread id for message {message_id}"))?;

        let next_leaf_id: i64 = tx.query_row(
            r#"
                INSERT INTO messages(thread_id, role, content, item_json, created_at, parent_entry_id)
                SELECT ?1, ?2, ?3, ?4, ?5, ?6
                RETURNING id
            "#, params![thread_id, role, content, item_json, created_at, message_id], |row| row.get(0)
        ).with_context(|| format!("failed to fork at message for thread {thread_id:?}"))?;

        tx.execute(
            r#"
            UPDATE threads
            SET current_leaf_id = ?1
            WHERE id = ?2;
            "#,
            params![next_leaf_id, thread_id],
        )
        .with_context(|| {
            format!("failed to update thread current leaf id for thread {thread_id:?}")
        })?;

        tx.commit()
            .context("failed to commit fork message transaction")?;

        Ok(next_leaf_id)
    }

    /// Delete all messages belonging to a thread and reset its `current_leaf_id`.
    ///
    /// Returns the number of messages deleted.
    pub fn clear_messages(&self, thread_id: &str) -> Result<usize> {
        let mut conn = self.conn()?;
        let tx = conn
            .transaction()
            .context("failed to begin clear messages transaction")?;

        tx.execute(
            r#"
            UPDATE threads
            SET current_leaf_id = NULL
            WHERE id = ?1;
            "#,
            params![thread_id],
        )
        .with_context(|| format!("failed to clear messages for thread {thread_id}"))?;
        let result = tx
            .execute(
                r#"
                DELETE FROM messages WHERE thread_id = ?1
                "#,
                params![thread_id],
            )
            .with_context(|| format!("failed to clear messages for thread {thread_id}"))?;
        tx.commit()
            .context("failed to commit clear messages transaction")?;

        Ok(result)
    }

    /// Save (or update) a named checkpoint for a thread.
    ///
    /// If a checkpoint with the same `thread_id` and `checkpoint_id` already exists,
    /// its state and timestamp are overwritten.
    pub fn save_checkpoint(
        &self,
        thread_id: &str,
        checkpoint_id: &str,
        state: &Value,
    ) -> Result<()> {
        let conn = self.conn()?;
        let state_json =
            serde_json::to_string(state).context("failed to encode checkpoint state")?;
        conn.execute(
            r#"
            INSERT INTO checkpoints(thread_id, checkpoint_id, state_json, created_at)
            VALUES (?1, ?2, ?3, ?4)
            ON CONFLICT(thread_id, checkpoint_id) DO UPDATE SET
                state_json = excluded.state_json,
                created_at = excluded.created_at
            "#,
            params![thread_id, checkpoint_id, state_json, Utc::now().timestamp()],
        )
        .with_context(|| {
            format!("failed to save checkpoint {checkpoint_id} for thread {thread_id}")
        })?;
        Ok(())
    }

    /// Load a checkpoint for a thread.
    ///
    /// If `checkpoint_id` is provided, loads that specific checkpoint. Otherwise,
    /// loads the most recently created checkpoint for the thread. Returns `None`
    /// if no matching checkpoint exists.
    pub fn load_checkpoint(
        &self,
        thread_id: &str,
        checkpoint_id: Option<&str>,
    ) -> Result<Option<CheckpointRecord>> {
        let conn = self.conn()?;
        if let Some(checkpoint_id) = checkpoint_id {
            let row = conn
                .query_row(
                    "SELECT thread_id, checkpoint_id, state_json, created_at FROM checkpoints WHERE thread_id = ?1 AND checkpoint_id = ?2",
                    params![thread_id, checkpoint_id],
                    |row| {
                        let state_json: String = row.get(2)?;
                        let state = serde_json::from_str(&state_json).unwrap_or(Value::Null);
                        Ok(CheckpointRecord {
                            thread_id: row.get(0)?,
                            checkpoint_id: row.get(1)?,
                            state,
                            created_at: row.get(3)?,
                        })
                    },
                )
                .optional()
                .with_context(|| {
                    format!("failed to load checkpoint {checkpoint_id} for thread {thread_id}")
                })?;
            return Ok(row);
        }

        conn.query_row(
            "SELECT thread_id, checkpoint_id, state_json, created_at FROM checkpoints WHERE thread_id = ?1 ORDER BY created_at DESC LIMIT 1",
            params![thread_id],
            |row| {
                let state_json: String = row.get(2)?;
                let state = serde_json::from_str(&state_json).unwrap_or(Value::Null);
                Ok(CheckpointRecord {
                    thread_id: row.get(0)?,
                    checkpoint_id: row.get(1)?,
                    state,
                    created_at: row.get(3)?,
                })
            },
        )
        .optional()
        .with_context(|| format!("failed to load latest checkpoint for thread {thread_id}"))
    }

    /// List checkpoints for a thread, ordered by creation time (newest first).
    ///
    /// The `limit` parameter caps the number of results and defaults to 100.
    pub fn list_checkpoints(
        &self,
        thread_id: &str,
        limit: Option<usize>,
    ) -> Result<Vec<CheckpointRecord>> {
        let conn = self.conn()?;
        let limit = i64::try_from(limit.unwrap_or(100)).unwrap_or(100);
        let mut stmt = conn
            .prepare(
                "SELECT thread_id, checkpoint_id, state_json, created_at FROM checkpoints WHERE thread_id = ?1 ORDER BY created_at DESC LIMIT ?2",
            )
            .context("failed to prepare checkpoint list query")?;
        let mut rows = stmt
            .query(params![thread_id, limit])
            .with_context(|| format!("failed to list checkpoints for thread {thread_id}"))?;

        let mut out = Vec::new();
        while let Some(row) = rows.next().context("failed to iterate checkpoint rows")? {
            let state_json: String = row.get(2).context("failed to read checkpoint state json")?;
            let state = serde_json::from_str(&state_json).unwrap_or(Value::Null);
            out.push(CheckpointRecord {
                thread_id: row.get(0).context("failed to read checkpoint thread id")?,
                checkpoint_id: row.get(1).context("failed to read checkpoint id")?,
                state,
                created_at: row.get(3).context("failed to read checkpoint timestamp")?,
            });
        }
        Ok(out)
    }

    /// Delete a specific checkpoint from a thread.
    pub fn delete_checkpoint(&self, thread_id: &str, checkpoint_id: &str) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "DELETE FROM checkpoints WHERE thread_id = ?1 AND checkpoint_id = ?2",
            params![thread_id, checkpoint_id],
        )
        .with_context(|| {
            format!("failed to delete checkpoint {checkpoint_id} for thread {thread_id}")
        })?;
        Ok(())
    }

    /// Insert or update a background job record.
    pub fn upsert_job(&self, job: &JobStateRecord) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            r#"
            INSERT INTO jobs(id, name, status, progress, detail, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                status = excluded.status,
                progress = excluded.progress,
                detail = excluded.detail,
                created_at = excluded.created_at,
                updated_at = excluded.updated_at
            "#,
            params![
                job.id,
                job.name,
                job_state_status_to_str(&job.status),
                job.progress.map(i64::from),
                job.detail,
                job.created_at,
                job.updated_at
            ],
        )
        .with_context(|| format!("failed to upsert job {}", job.id))?;
        Ok(())
    }

    /// Retrieve a single job by its ID.
    ///
    /// Returns `None` if no job with the given ID exists.
    pub fn get_job(&self, id: &str) -> Result<Option<JobStateRecord>> {
        let conn = self.conn()?;
        conn.query_row(
            "SELECT id, name, status, progress, detail, created_at, updated_at FROM jobs WHERE id = ?1",
            params![id],
            |row| {
                let status_raw: String = row.get(2)?;
                let progress: Option<i64> = row.get(3)?;
                Ok(JobStateRecord {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    status: job_state_status_from_str(&status_raw),
                    progress: progress.and_then(|v| u8::try_from(v).ok()),
                    detail: row.get(4)?,
                    created_at: row.get(5)?,
                    updated_at: row.get(6)?,
                })
            },
        )
        .optional()
        .with_context(|| format!("failed to read job {id}"))
    }

    /// List jobs ordered by most recently updated.
    ///
    /// The `limit` parameter caps the number of results and defaults to 100.
    pub fn list_jobs(&self, limit: Option<usize>) -> Result<Vec<JobStateRecord>> {
        let conn = self.conn()?;
        let limit = i64::try_from(limit.unwrap_or(100)).unwrap_or(100);
        let mut stmt = conn
            .prepare(
                "SELECT id, name, status, progress, detail, created_at, updated_at FROM jobs ORDER BY updated_at DESC LIMIT ?1",
            )
            .context("failed to prepare job list query")?;
        let mut rows = stmt
            .query(params![limit])
            .context("failed to query persisted jobs")?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().context("failed to iterate persisted jobs")? {
            let status_raw: String = row.get(2).context("failed to read job status")?;
            let progress: Option<i64> = row.get(3).context("failed to read job progress")?;
            out.push(JobStateRecord {
                id: row.get(0).context("failed to read job id")?,
                name: row.get(1).context("failed to read job name")?,
                status: job_state_status_from_str(&status_raw),
                progress: progress.and_then(|v| u8::try_from(v).ok()),
                detail: row.get(4).context("failed to read job detail")?,
                created_at: row.get(5).context("failed to read job created_at")?,
                updated_at: row.get(6).context("failed to read job updated_at")?,
            });
        }
        Ok(out)
    }

    /// Permanently delete a job record.
    pub fn delete_job(&self, id: &str) -> Result<()> {
        let conn = self.conn()?;
        conn.execute("DELETE FROM jobs WHERE id = ?1", params![id])
            .with_context(|| format!("failed to delete job {id}"))?;
        Ok(())
    }

    /// Look up the rollout file path for a thread by its ID.
    pub fn find_rollout_path_by_id(&self, id: &str) -> Result<Option<PathBuf>> {
        let conn = self.conn()?;
        conn.query_row(
            "SELECT rollout_path FROM threads WHERE id = ?1",
            params![id],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
        .context("failed to lookup rollout path")
        .map(|opt| opt.flatten().map(PathBuf::from))
    }

    /// Default on-disk location for a thread's JSONL transcript, derived from
    /// the state DB location: `<state_dir>/rollouts/<thread_id>.jsonl`.
    ///
    /// Used when a thread is created without an explicit `rollout_path`, so
    /// every persisted thread gets a structured transcript copy.
    pub fn default_rollout_path(&self, thread_id: &str) -> Option<PathBuf> {
        let dir = self.db_path.parent()?;
        Some(dir.join("rollouts").join(format!("{thread_id}.jsonl")))
    }

    /// Append an entry to the JSONL session index file.
    ///
    /// The session index is an append-only log that maps thread IDs to their names,
    /// update timestamps, and rollout paths. It is used for fast name-based lookups
    /// without opening the SQLite database.
    pub fn append_thread_name(
        &self,
        thread_id: &str,
        thread_name: Option<String>,
        updated_at: i64,
        rollout_path: Option<PathBuf>,
    ) -> Result<()> {
        if let Some(parent) = self.session_index_path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "failed to create session index directory {}",
                    parent.display()
                )
            })?;
        }
        let entry = SessionIndexEntry {
            thread_id: thread_id.to_string(),
            thread_name,
            updated_at,
            rollout_path,
        };
        let encoded =
            serde_json::to_string(&entry).context("failed to serialize session index entry")?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.session_index_path)
            .with_context(|| {
                format!(
                    "failed to open session index {}",
                    self.session_index_path.display()
                )
            })?;
        writeln!(file, "{encoded}").context("failed to append session index entry")?;
        Ok(())
    }

    /// Find the display name for a thread by its ID, using the session index.
    ///
    /// Returns `None` if the thread is not in the index or has no name.
    pub fn find_thread_name_by_id(&self, thread_id: &str) -> Result<Option<String>> {
        let map = self.session_index_map()?;
        Ok(map
            .get(thread_id)
            .and_then(|entry| entry.thread_name.clone()))
    }

    /// Look up display names for multiple thread IDs at once.
    ///
    /// Returns a map from thread ID to its name (which may be `None`).
    pub fn find_thread_names_by_ids(
        &self,
        ids: &[String],
    ) -> Result<HashMap<String, Option<String>>> {
        let map = self.session_index_map()?;
        let mut out = HashMap::new();
        for id in ids {
            let name = map.get(id).and_then(|entry| entry.thread_name.clone());
            out.insert(id.clone(), name);
        }
        Ok(out)
    }

    /// Find the rollout path for a thread by its display name (case-insensitive).
    ///
    /// If multiple threads share the same name, the most recently updated one is returned.
    /// Returns `None` if no matching thread is found.
    pub fn find_thread_path_by_name_str(&self, name: &str) -> Result<Option<PathBuf>> {
        let map = self.session_index_map()?;
        let matched = map
            .values()
            .filter(|entry| {
                entry
                    .thread_name
                    .as_deref()
                    .is_some_and(|n| n.eq_ignore_ascii_case(name))
            })
            .max_by_key(|entry| entry.updated_at);
        Ok(matched.and_then(|entry| entry.rollout_path.clone()))
    }

    fn session_index_map(&self) -> Result<HashMap<String, SessionIndexEntry>> {
        if !self.session_index_path.exists() {
            return Ok(HashMap::new());
        }
        let file = OpenOptions::new()
            .read(true)
            .open(&self.session_index_path)
            .with_context(|| {
                format!(
                    "failed to read session index {}",
                    self.session_index_path.display()
                )
            })?;
        let reader = BufReader::new(file);
        let mut latest = HashMap::<String, SessionIndexEntry>::new();
        for line in reader.lines() {
            let line = line.context("failed to read session index line")?;
            if line.trim().is_empty() {
                continue;
            }
            let parsed: SessionIndexEntry =
                serde_json::from_str(&line).context("failed to parse session index entry")?;
            latest.insert(parsed.thread_id.clone(), parsed);
        }
        Ok(latest)
    }
}

pub fn default_state_db_path() -> PathBuf {
    // $MIMOFAN_HOME is a hard override of the base data directory
    // (docs/CONFIGURATION.md): when set, the state DB lives under it and we do
    // NOT fall back to the legacy ~/.mimofan path — silent fallback would
    // defeat the isolation the override promises (CI, containers, multi-project,
    // test harnesses). `mimofan_config::resolve_state_dir` honors this override
    // as the single source of truth.
    mimofan_config::resolve_state_dir("state.db")
        .unwrap_or_else(|_| PathBuf::from(".").join(".mimofan").join("state.db"))
}

fn bool_to_i64(value: bool) -> i64 {
    if value { 1 } else { 0 }
}

fn i64_to_bool(value: i64) -> bool {
    value != 0
}

fn thread_status_to_str(status: &ThreadStatus) -> &'static str {
    match status {
        ThreadStatus::Running => "running",
        ThreadStatus::Idle => "idle",
        ThreadStatus::Completed => "completed",
        ThreadStatus::Failed => "failed",
        ThreadStatus::Paused => "paused",
        ThreadStatus::Archived => "archived",
    }
}

fn thread_status_from_str(value: &str) -> ThreadStatus {
    match value {
        "running" => ThreadStatus::Running,
        "idle" => ThreadStatus::Idle,
        "completed" => ThreadStatus::Completed,
        "failed" => ThreadStatus::Failed,
        "paused" => ThreadStatus::Paused,
        "archived" => ThreadStatus::Archived,
        _ => ThreadStatus::Idle,
    }
}

fn session_source_to_str(source: &SessionSource) -> &'static str {
    match source {
        SessionSource::Interactive => "interactive",
        SessionSource::Resume => "resume",
        SessionSource::Fork => "fork",
        SessionSource::Api => "api",
        SessionSource::Unknown => "unknown",
    }
}

fn session_source_from_str(value: &str) -> SessionSource {
    match value {
        "interactive" => SessionSource::Interactive,
        "resume" => SessionSource::Resume,
        "fork" => SessionSource::Fork,
        "api" => SessionSource::Api,
        _ => SessionSource::Unknown,
    }
}

fn path_to_opt_string(path: Option<&Path>) -> Option<String> {
    path.map(|p| p.display().to_string())
}

fn job_state_status_to_str(status: &JobStateStatus) -> &'static str {
    match status {
        JobStateStatus::Queued => "queued",
        JobStateStatus::Running => "running",
        JobStateStatus::Completed => "completed",
        JobStateStatus::Failed => "failed",
        JobStateStatus::Cancelled => "cancelled",
    }
}

fn job_state_status_from_str(value: &str) -> JobStateStatus {
    match value {
        "queued" => JobStateStatus::Queued,
        "running" => JobStateStatus::Running,
        "completed" => JobStateStatus::Completed,
        "failed" => JobStateStatus::Failed,
        "cancelled" => JobStateStatus::Cancelled,
        _ => JobStateStatus::Queued,
    }
}

fn thread_goal_status_to_str(status: &ThreadGoalStatus) -> &'static str {
    match status {
        ThreadGoalStatus::Active => "active",
        ThreadGoalStatus::Paused => "paused",
        ThreadGoalStatus::Blocked => "blocked",
        ThreadGoalStatus::UsageLimited => "usage_limited",
        ThreadGoalStatus::BudgetLimited => "budget_limited",
        ThreadGoalStatus::Complete => "complete",
    }
}

fn thread_goal_status_from_str(value: &str) -> ThreadGoalStatus {
    match value {
        "active" => ThreadGoalStatus::Active,
        "paused" => ThreadGoalStatus::Paused,
        "blocked" => ThreadGoalStatus::Blocked,
        "usage_limited" => ThreadGoalStatus::UsageLimited,
        "budget_limited" => ThreadGoalStatus::BudgetLimited,
        "complete" => ThreadGoalStatus::Complete,
        _ => ThreadGoalStatus::Active,
    }
}

fn row_to_thread(row: &rusqlite::Row<'_>) -> rusqlite::Result<ThreadMetadata> {
    let status_raw: String = row.get(7)?;
    let source_raw: String = row.get(11)?;
    let rollout_path: Option<String> = row.get(1)?;
    let path: Option<String> = row.get(8)?;
    Ok(ThreadMetadata {
        id: row.get(0)?,
        rollout_path: rollout_path.map(PathBuf::from),
        preview: row.get(2)?,
        ephemeral: i64_to_bool(row.get(3)?),
        model_provider: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
        status: thread_status_from_str(&status_raw),
        path: path.map(PathBuf::from),
        cwd: PathBuf::from(row.get::<_, String>(9)?),
        cli_version: row.get(10)?,
        source: session_source_from_str(&source_raw),
        name: row.get(12)?,
        sandbox_policy: row.get(13)?,
        approval_mode: row.get(14)?,
        archived: i64_to_bool(row.get(15)?),
        archived_at: row.get(16)?,
        git_sha: row.get(17)?,
        git_branch: row.get(18)?,
        git_origin_url: row.get(19)?,
        memory_mode: row.get(20)?,
        current_leaf_id: row.get(21)?,
    })
}

fn row_to_thread_goal(row: &rusqlite::Row<'_>) -> rusqlite::Result<ThreadGoalRecord> {
    let status_raw: String = row.get(3)?;
    Ok(ThreadGoalRecord {
        thread_id: row.get(0)?,
        goal_id: row.get(1)?,
        objective: row.get(2)?,
        status: thread_goal_status_from_str(&status_raw),
        token_budget: row.get(4)?,
        tokens_used: row.get(5)?,
        time_used_seconds: row.get(6)?,
        continuation_count: row.get(7)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_db_path() -> PathBuf {
        // Unique per-call name without pulling in a uuid dependency.
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "mimofan_state_test_{}_{}_{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0),
            n
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("state.db")
    }

    fn sample_thread(id: &str) -> ThreadMetadata {
        let now = chrono::Utc::now().timestamp();
        ThreadMetadata {
            id: id.to_string(),
            rollout_path: None, // let the store assign the default
            preview: "test".to_string(),
            ephemeral: false,
            model_provider: "openai".to_string(),
            created_at: now,
            updated_at: now,
            status: ThreadStatus::Running,
            path: None,
            cwd: PathBuf::from("."),
            cli_version: "test".to_string(),
            source: SessionSource::Interactive,
            name: None,
            sandbox_policy: None,
            approval_mode: None,
            archived: false,
            archived_at: None,
            git_sha: None,
            git_branch: None,
            git_origin_url: None,
            memory_mode: None,
            current_leaf_id: None,
        }
    }

    #[test]
    fn new_thread_gets_default_rollout_path() {
        let db = tmp_db_path();
        let store = StateStore::open(Some(db.clone())).unwrap();
        store.upsert_thread(&sample_thread("thread-abc")).unwrap();
        let path = store.find_rollout_path_by_id("thread-abc").unwrap();
        assert!(path.is_some(), "rollout_path should be auto-assigned");
        let p = path.unwrap();
        assert!(p.to_string_lossy().ends_with("thread-abc.jsonl"));
        let _ = std::fs::remove_dir_all(db.parent().unwrap());
    }

    #[test]
    fn append_message_writes_jsonl_transcript() {
        let db = tmp_db_path();
        let store = StateStore::open(Some(db.clone())).unwrap();
        store.upsert_thread(&sample_thread("thread-xyz")).unwrap();

        store
            .append_message("thread-xyz", "user", "hello", None)
            .unwrap();
        store
            .append_message(
                "thread-xyz",
                "assistant",
                "hi there",
                Some(serde_json::json!({"kind": "text"})),
            )
            .unwrap();

        // read_transcript should return both lines as valid JSON.
        let transcript = store.read_transcript("thread-xyz").unwrap();
        let lines = transcript.expect("transcript should exist");
        assert_eq!(lines.len(), 2, "two messages -> two transcript lines");
        assert_eq!(lines[0]["role"], "user");
        assert_eq!(lines[0]["content"], "hello");
        assert_eq!(lines[1]["role"], "assistant");
        assert_eq!(lines[1]["item"]["kind"], "text");

        // The raw file must be valid newline-delimited JSON.
        let path = store
            .find_rollout_path_by_id("thread-xyz")
            .unwrap()
            .unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        assert_eq!(raw.lines().count(), 2);
        for line in raw.lines() {
            serde_json::from_str::<serde_json::Value>(line).unwrap();
        }
        let _ = std::fs::remove_dir_all(db.parent().unwrap());
    }

    #[test]
    fn read_transcript_returns_none_for_unknown_thread() {
        let db = tmp_db_path();
        let store = StateStore::open(Some(db.clone())).unwrap();
        assert!(store.read_transcript("nope").unwrap().is_none());
        let _ = std::fs::remove_dir_all(db.parent().unwrap());
    }
}
