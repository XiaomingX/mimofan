//! Task delegation system

// WIP collaboration surface — narrow harvest. See `tab/mod.rs` for the
// PR #2753 context.
#![allow(dead_code)]

use super::{Priority, TabId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Status of a delegation task
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DelegationStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
    Cancelled,
}

/// A delegation task
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegationTask {
    pub task_id: String,
    pub from_tab: TabId,
    pub to_tab: TabId,
    pub description: String,
    pub priority: Priority,
    pub status: DelegationStatus,
    pub created_at: DateTime<Utc>,
    pub deadline: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub result: Option<String>,
}

impl DelegationTask {
    /// Create a new pending delegation task.
    ///
    /// # Arguments
    /// * `task_id` - Unique identifier (e.g. "delegation_42")
    /// * `from` - Tab that originated the task
    /// * `to` - Tab that should execute the task
    /// * `description` - Human-readable description of what to do
    /// * `priority` - Priority level (Low/Normal/High/Urgent)
    pub fn new(
        task_id: String,
        from: TabId,
        to: TabId,
        description: String,
        priority: Priority,
    ) -> Self {
        Self {
            task_id,
            from_tab: from,
            to_tab: to,
            description,
            priority,
            status: DelegationStatus::Pending,
            created_at: Utc::now(),
            deadline: None,
            completed_at: None,
            result: None,
        }
    }

    /// Builder method: set the deadline for this task. Returns self for chaining.
    pub fn with_deadline(mut self, deadline: DateTime<Utc>) -> Self {
        self.deadline = Some(deadline);
        self
    }

    /// Transition status to InProgress. Idempotent.
    pub fn start(&mut self) {
        self.status = DelegationStatus::InProgress;
    }

    /// Mark as completed with the given result string.
    /// Records completion timestamp.
    pub fn complete(&mut self, result: String) {
        self.status = DelegationStatus::Completed;
        self.result = Some(result);
        self.completed_at = Some(Utc::now());
    }

    /// Mark as failed (no result). Records completion timestamp.
    pub fn fail(&mut self) {
        self.status = DelegationStatus::Failed;
        self.completed_at = Some(Utc::now());
    }

    /// Cancel the task. Records completion timestamp.
    pub fn cancel(&mut self) {
        self.status = DelegationStatus::Cancelled;
        self.completed_at = Some(Utc::now());
    }

    /// Returns true if the task is still pending (not yet started).
    pub fn is_pending(&self) -> bool {
        self.status == DelegationStatus::Pending
    }

    /// Returns true if the task completed successfully.
    pub fn is_completed(&self) -> bool {
        self.status == DelegationStatus::Completed
    }

    /// Returns true if the task is pending or in progress (i.e., not terminal).
    pub fn is_active(&self) -> bool {
        matches!(
            self.status,
            DelegationStatus::Pending | DelegationStatus::InProgress
        )
    }
}

/// Result of a completed delegation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegationResult {
    pub task_id: String,
    pub from_tab: TabId,
    pub to_tab: TabId,
    pub result: String,
    pub completed_at: DateTime<Utc>,
    pub was_successful: bool,
}

/// Task delegator managing cross-tab task distribution
pub struct TaskDelegator {
    /// Active tasks (pending + in-progress). Terminal-state tasks
    /// (completed / failed / cancelled) are removed from this vec and
    /// recorded in `completed_results`. `pub(crate)` so the persistence
    /// layer can restore from snapshot.
    pub(crate) pending_tasks: Vec<DelegationTask>,
    /// Bounded ring buffer of completed results.
    /// Using VecDeque so O(1) front removal when pruning old entries.
    /// Bounded to MAX_COMPLETED_RESULTS to prevent unbounded growth.
    completed_results: VecDeque<DelegationResult>,
    next_id: u64,
}

/// Maximum number of completed results to keep in memory.
/// At this size, prune_results is a no-op for the common case.
const MAX_COMPLETED_RESULTS: usize = 256;

impl TaskDelegator {
    pub fn new() -> Self {
        Self {
            pending_tasks: Vec::new(),
            completed_results: VecDeque::new(),
            next_id: 1,
        }
    }

    /// Create a new delegation
    pub fn create_delegation(
        &mut self,
        from: TabId,
        to: TabId,
        description: String,
        priority: Priority,
    ) -> Option<String> {
        let task_id = self.generate_task_id();
        let task = DelegationTask::new(task_id.clone(), from, to, description, priority);
        self.pending_tasks.push(task);
        Some(task_id)
    }

    /// Create a delegation with a deadline
    pub fn create_delegation_with_deadline(
        &mut self,
        from: TabId,
        to: TabId,
        description: String,
        priority: Priority,
        deadline: DateTime<Utc>,
    ) -> Option<String> {
        let task_id = self.generate_task_id();
        let task = DelegationTask::new(task_id.clone(), from, to, description, priority)
            .with_deadline(deadline);
        self.pending_tasks.push(task);
        Some(task_id)
    }

    /// Get pending tasks for a tab
    pub fn pending_for_tab(&self, tab_id: TabId) -> Vec<&DelegationTask> {
        self.pending_tasks
            .iter()
            .filter(|t| t.to_tab == tab_id && t.is_pending())
            .collect()
    }

    /// Get active tasks for a tab (pending or in progress)
    pub fn active_for_tab(&self, tab_id: TabId) -> Vec<&DelegationTask> {
        self.pending_tasks
            .iter()
            .filter(|t| t.to_tab == tab_id && t.is_active())
            .collect()
    }

    /// Get all pending tasks
    pub fn all_pending(&self) -> &[DelegationTask] {
        &self.pending_tasks
    }

    /// Get pending tasks sorted by priority (highest first)
    pub fn pending_sorted_by_priority(&self) -> Vec<&DelegationTask> {
        let mut tasks: Vec<&DelegationTask> = self
            .pending_tasks
            .iter()
            .filter(|t| t.is_pending())
            .collect();
        tasks.sort_by_key(|t| std::cmp::Reverse(t.priority));
        tasks
    }

    /// Start working on a task
    pub fn start_task(&mut self, task_id: &str) -> bool {
        if let Some(task) = self.pending_tasks.iter_mut().find(|t| t.task_id == task_id) {
            task.start();
            true
        } else {
            false
        }
    }

    /// Take the highest-priority pending task for a tab.
    /// Marks the task as `InProgress` in place and returns a clone; the task
    /// is only removed from the queue when it reaches a terminal state via
    /// `complete` / `fail_task` / `cancel_task`. Higher priority wins; on tie,
    /// earlier `created_at` wins.
    pub fn take_pending_for_tab(&mut self, tab_id: TabId) -> Option<DelegationTask> {
        // Find the highest priority pending task for this tab
        let mut best_idx: Option<usize> = None;
        for (i, task) in self.pending_tasks.iter().enumerate() {
            if task.to_tab != tab_id || !task.is_pending() {
                continue;
            }
            match best_idx {
                None => best_idx = Some(i),
                Some(b) => {
                    let best = &self.pending_tasks[b];
                    // Higher priority wins; if equal, earlier created_at wins
                    if task.priority > best.priority
                        || (task.priority == best.priority && task.created_at < best.created_at)
                    {
                        best_idx = Some(i);
                    }
                }
            }
        }

        // Mark as in-progress in place and return a clone; do NOT remove.
        best_idx.map(|i| {
            self.pending_tasks[i].start();
            self.pending_tasks[i].clone()
        })
    }

    /// Peek at the next pending task for a tab without removing it
    pub fn peek_pending_for_tab(&self, tab_id: TabId) -> Option<&DelegationTask> {
        self.pending_tasks
            .iter()
            .filter(|t| t.to_tab == tab_id && t.is_pending())
            .max_by(|a, b| {
                // Higher priority first; on tie, earlier created_at first
                a.priority
                    .cmp(&b.priority)
                    .then_with(|| b.created_at.cmp(&a.created_at))
            })
    }

    /// Complete a task
    pub fn complete(&mut self, task_id: &str, result: String) {
        let pos = self.pending_tasks.iter().position(|t| t.task_id == task_id);
        let Some(pos) = pos else { return };
        let mut task = self.pending_tasks.swap_remove(pos);
        let from = task.from_tab;
        let to = task.to_tab;
        task.complete(result.clone());

        self.completed_results.push_back(DelegationResult {
            task_id: task_id.to_string(),
            from_tab: from,
            to_tab: to,
            result,
            completed_at: Utc::now(),
            was_successful: true,
        });
        // Auto-prune to bound memory
        if self.completed_results.len() > MAX_COMPLETED_RESULTS {
            self.completed_results.pop_front();
        }
    }

    /// Fail a task
    pub fn fail_task(&mut self, task_id: &str) {
        let pos = self.pending_tasks.iter().position(|t| t.task_id == task_id);
        let Some(pos) = pos else { return };
        let mut task = self.pending_tasks.swap_remove(pos);
        let from = task.from_tab;
        let to = task.to_tab;
        task.fail();

        self.completed_results.push_back(DelegationResult {
            task_id: task_id.to_string(),
            from_tab: from,
            to_tab: to,
            result: String::new(),
            completed_at: Utc::now(),
            was_successful: false,
        });
        // Auto-prune to bound memory
        if self.completed_results.len() > MAX_COMPLETED_RESULTS {
            self.completed_results.pop_front();
        }
    }

    /// Cancel a task
    pub fn cancel_task(&mut self, task_id: &str) -> bool {
        let Some(pos) = self.pending_tasks.iter().position(|t| t.task_id == task_id) else {
            return false;
        };
        let mut task = self.pending_tasks.swap_remove(pos);
        task.cancel();
        true
    }

    /// Get results for a tab
    pub fn results_for_tab(&self, tab_id: TabId) -> Vec<&DelegationResult> {
        self.completed_results
            .iter()
            .filter(|r| r.to_tab == tab_id)
            .collect()
    }

    /// Get pending count for a tab
    pub fn pending_count(&self, tab_id: TabId) -> usize {
        self.pending_tasks
            .iter()
            .filter(|t| t.to_tab == tab_id && t.is_pending())
            .count()
    }

    /// Clean up old completed results (keep last N)
    /// O(N) where N is the number of items to remove, but much faster than
    /// the previous drain() implementation because VecDeque supports
    /// O(1) front removal.
    pub fn prune_results(&mut self, keep_last: usize) {
        while self.completed_results.len() > keep_last {
            self.completed_results.pop_front();
        }
    }

    /// Get completed results sorted by completion time (most recent first)
    pub fn recent_results(&self, limit: usize) -> Vec<&DelegationResult> {
        let mut results: Vec<&DelegationResult> = self.completed_results.iter().collect();
        results.sort_by_key(|r| std::cmp::Reverse(r.completed_at));
        results.into_iter().take(limit).collect()
    }

    fn generate_task_id(&mut self) -> String {
        let id = self.next_id;
        self.next_id += 1;
        format!("delegation_{id}")
    }

    pub(crate) fn advance_next_id_past_existing_tasks(&mut self) {
        let max_seen = self
            .pending_tasks
            .iter()
            .filter_map(|task| task.task_id.strip_prefix("delegation_"))
            .filter_map(|suffix| suffix.parse::<u64>().ok())
            .max()
            .unwrap_or(0);
        self.next_id = self.next_id.max(max_seen + 1);
    }
}

impl Default for TaskDelegator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {}
