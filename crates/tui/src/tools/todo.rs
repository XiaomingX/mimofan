//! Todo list tool and supporting data structures.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

use async_trait::async_trait;
use chrono::Utc;
use fd_lock::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::tools::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec,
};

// === Types ===

/// Status for a todo item.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
    /// A failed task that has been auto-degraded into finer subtasks. The
    /// parent is no longer eligible for bare retry or re-scheduling; it waits
    /// for its generated subtasks to complete, at which point it is resolved.
    Degraded,
}

impl TodoStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            TodoStatus::Pending => "pending",
            TodoStatus::InProgress => "in_progress",
            TodoStatus::Completed => "completed",
            TodoStatus::Degraded => "degraded",
        }
    }

    /// Parse a string into a todo status.
    #[must_use]
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(value: &str) -> Option<Self> {
        match value.trim().to_lowercase().as_str() {
            "pending" => Some(TodoStatus::Pending),
            "in_progress" | "inprogress" => Some(TodoStatus::InProgress),
            "completed" | "done" => Some(TodoStatus::Completed),
            "degraded" => Some(TodoStatus::Degraded),
            _ => None,
        }
    }
}

/// A single todo item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    pub id: u32,
    pub content: String,
    pub status: TodoStatus,
    /// Ids of items that must reach `Completed` before this one can start.
    ///
    /// Defaults to empty and is skipped when serializing an unblocked item, so
    /// sessions saved before dependencies existed still deserialize cleanly.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked_by: Vec<u32>,
}

impl TodoItem {
    /// Whether this item is waiting on at least one unfinished dependency.
    #[must_use]
    pub fn is_blocked(&self, items: &[TodoItem]) -> bool {
        !self.unmet_dependencies(items).is_empty()
    }

    /// Dependency ids that are not yet completed. Ids that no longer exist in
    /// the list are treated as satisfied rather than as a permanent block, so
    /// deleting an upstream item cannot strand its dependents.
    #[must_use]
    pub fn unmet_dependencies(&self, items: &[TodoItem]) -> Vec<u32> {
        self.blocked_by
            .iter()
            .copied()
            .filter(|dep| {
                items
                    .iter()
                    .find(|item| item.id == *dep)
                    .is_some_and(|item| item.status != TodoStatus::Completed)
            })
            .collect()
    }
}

/// Snapshot of a todo list for display or serialization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoListSnapshot {
    pub items: Vec<TodoItem>,
    pub completion_pct: u8,
    pub in_progress_id: Option<u32>,
    /// Pending items whose dependencies are all satisfied — the set an agent
    /// may claim right now.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ready_ids: Vec<u32>,
    /// Pending items still waiting on an unfinished dependency.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked_ids: Vec<u32>,
}

/// Mutable list of todo items with helper operations.
#[derive(Debug, Clone, Default)]
pub struct TodoList {
    items: Vec<TodoItem>,
    next_id: u32,
    /// In-memory advisory claim ownership (`task_id` → `agent_id`). The
    /// cross-process source of truth lives in `TodoClaimStore`; this field is
    /// only the single-process fallback used when no durable store is wired.
    claims: HashMap<u32, String>,
    /// Maps a failed-and-degraded task id to the ids of the finer subtasks that
    /// replaced it. Used to (a) resolve the parent once all subtasks complete
    /// and (b) let the job-retry layer skip a bare retry for a degraded task.
    degraded_children: HashMap<u32, Vec<u32>>,
}

impl TodoList {
    /// Create an empty todo list.
    #[must_use]
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            next_id: 1,
            claims: HashMap::new(),
            degraded_children: HashMap::new(),
        }
    }

    /// Look up a todo item by id.
    #[must_use]
    pub fn get(&self, id: u32) -> Option<&TodoItem> {
        self.items.iter().find(|item| item.id == id)
    }

    /// Ids of subtasks generated when `parent_id` was degraded, if any.
    #[must_use]
    pub fn degraded_subtasks(&self, parent_id: u32) -> Option<&Vec<u32>> {
        self.degraded_children.get(&parent_id)
    }

    /// Whether `id` has been degraded into subtasks (and therefore must not be
    /// bare-retried or re-scheduled as a standalone task).
    #[must_use]
    pub fn is_degraded(&self, id: u32) -> bool {
        self.degraded_children.contains_key(&id)
    }

    /// Return a snapshot of the list with computed metrics.
    #[must_use]
    pub fn snapshot(&self) -> TodoListSnapshot {
        TodoListSnapshot {
            items: self.items.clone(),
            completion_pct: self.completion_percentage(),
            in_progress_id: self.in_progress_id(),
            ready_ids: self.ready_ids(),
            blocked_ids: self.blocked_ids(),
        }
    }

    /// Add a new todo item.
    pub fn add(&mut self, content: String, status: TodoStatus) -> TodoItem {
        self.add_with_dependencies(content, status, Vec::new())
    }

    /// Add a new todo item that waits on `blocked_by` before it can start.
    ///
    /// Dependencies are sanitized: self-references, unknown ids, duplicates,
    /// and any edge that would close a cycle are dropped, so the list can
    /// never deadlock itself.
    pub fn add_with_dependencies(
        &mut self,
        content: String,
        status: TodoStatus,
        blocked_by: Vec<u32>,
    ) -> TodoItem {
        let id = self.next_id;
        let blocked_by = self.sanitize_dependencies(id, blocked_by);

        let status = match status {
            TodoStatus::InProgress => {
                self.set_single_in_progress(None);
                TodoStatus::InProgress
            }
            other => other,
        };

        let item = TodoItem {
            id,
            content,
            status,
            blocked_by,
        };
        self.next_id += 1;
        self.items.push(item.clone());
        item
    }

    /// Ids of pending items that are ready to be claimed.
    #[must_use]
    pub fn ready_ids(&self) -> Vec<u32> {
        self.items
            .iter()
            .filter(|item| item.status == TodoStatus::Pending && !item.is_blocked(&self.items))
            .map(|item| item.id)
            .collect()
    }

    /// Ids of pending items still waiting on a dependency.
    #[must_use]
    pub fn blocked_ids(&self) -> Vec<u32> {
        self.items
            .iter()
            .filter(|item| item.status == TodoStatus::Pending && item.is_blocked(&self.items))
            .map(|item| item.id)
            .collect()
    }

    /// Unmet dependencies for `id`, or `None` if no such item exists.
    #[must_use]
    pub fn unmet_dependencies(&self, id: u32) -> Option<Vec<u32>> {
        self.items
            .iter()
            .find(|item| item.id == id)
            .map(|item| item.unmet_dependencies(&self.items))
    }

    /// Drop self-references, unknown ids, duplicates, and cycle-closing edges.
    fn sanitize_dependencies(&self, id: u32, requested: Vec<u32>) -> Vec<u32> {
        let mut accepted: Vec<u32> = Vec::new();
        for dep in requested {
            if dep == id
                || accepted.contains(&dep)
                || !self.items.iter().any(|item| item.id == dep)
                || self.depends_on(dep, id)
            {
                continue;
            }
            accepted.push(dep);
        }
        accepted
    }

    /// Whether `from` transitively depends on `target`.
    fn depends_on(&self, from: u32, target: u32) -> bool {
        let mut stack = vec![from];
        let mut seen: Vec<u32> = Vec::new();
        while let Some(current) = stack.pop() {
            if current == target {
                return true;
            }
            if seen.contains(&current) {
                continue;
            }
            seen.push(current);
            if let Some(item) = self.items.iter().find(|item| item.id == current) {
                stack.extend(item.blocked_by.iter().copied());
            }
        }
        false
    }

    /// Update an item's status by id.
    ///
    /// Starting a blocked item is rejected — see [`Self::try_update_status`]
    /// for the variant that reports *why*.
    pub fn update_status(&mut self, id: u32, status: TodoStatus) -> Option<TodoItem> {
        self.try_update_status(id, status).ok().flatten()
    }

    /// Update an item's status, refusing to move a still-blocked item into
    /// `InProgress` and returning the unmet dependency ids instead.
    ///
    /// This is what makes the dependency graph load-bearing rather than
    /// advisory: without it, an agent scanning the list could claim work whose
    /// prerequisites have not landed yet.
    pub fn try_update_status(
        &mut self,
        id: u32,
        status: TodoStatus,
    ) -> Result<Option<TodoItem>, Vec<u32>> {
        if status == TodoStatus::InProgress {
            let unmet = self
                .items
                .iter()
                .find(|item| item.id == id)
                .map(|item| item.unmet_dependencies(&self.items))
                .unwrap_or_default();
            if !unmet.is_empty() {
                return Err(unmet);
            }
        }
        Ok(self.apply_status(id, status))
    }

    fn apply_status(&mut self, id: u32, status: TodoStatus) -> Option<TodoItem> {
        let mut updated: Option<TodoItem> = None;
        if status == TodoStatus::InProgress {
            self.set_single_in_progress(Some(id));
        }
        for item in &mut self.items {
            if item.id == id {
                item.status = status;
                updated = Some(item.clone());
                break;
            }
        }
        updated
    }

    /// Auto-degrade a failed task into finer subtasks that re-enter the graph.
    ///
    /// The failed task is marked [`TodoStatus::Degraded`] so it is no longer
    /// eligible for bare retry or re-scheduling, and each `subtasks` entry is
    /// added as a fresh, ready-to-claim [`TodoStatus::Pending`] item. When a
    /// matching background `job` is supplied (`job_manager` + `job_id`), it is
    /// marked degraded as well so the job layer will not also run a bare retry
    /// — the two recovery systems stay mutually exclusive.
    ///
    /// Returns the ids of the generated subtasks, or `None` if `failed_id` does
    /// not exist or is not a terminal failure (already degraded / not failed).
    pub fn degrade_to_subtask(
        &mut self,
        failed_id: u32,
        subtasks: Vec<String>,
        job_manager: Option<&mut mimofan_core::JobManager>,
        job_id: Option<&str>,
    ) -> Option<Vec<u32>> {
        // Only a task that currently exists and is *not* already degraded can
        // be degraded again; do not double-degrade.
        let target = self.items.iter().find(|item| item.id == failed_id)?;
        if target.status == TodoStatus::Degraded {
            return None;
        }

        if subtasks.is_empty() {
            return None;
        }

        // Mark the parent as degraded (waiting on its subtasks).
        let mut child_ids: Vec<u32> = Vec::with_capacity(subtasks.len());
        for content in subtasks {
            let child = self.add_with_dependencies(content, TodoStatus::Pending, Vec::new());
            child_ids.push(child.id);
        }
        self.degraded_children.insert(failed_id, child_ids.clone());
        self.apply_status(failed_id, TodoStatus::Degraded);

        // Bridge with the job-retry layer: if this task maps to a background
        // job, mark it degraded so `JobManager::fail` will not schedule a bare
        // retry for an already-degraded task.
        if let (Some(manager), Some(job_id)) = (job_manager, job_id) {
            manager.mark_degraded(job_id);
        }

        Some(child_ids)
    }

    /// Resolve a degraded task: once every generated subtask is `Completed`,
    /// mark the parent `Completed` and drop the degradation edge.
    ///
    /// Returns the resolved parent id, or `None` if it is not yet resoluble.
    pub fn try_resolve_degraded(&mut self, parent_id: u32) -> Option<u32> {
        let Some(children) = self.degraded_children.get(&parent_id) else {
            return None;
        };
        let all_done = children.iter().all(|&cid| {
            self.items
                .iter()
                .find(|item| item.id == cid)
                .is_some_and(|item| item.status == TodoStatus::Completed)
        });
        if all_done {
            self.degraded_children.remove(&parent_id);
            self.apply_status(parent_id, TodoStatus::Completed);
            Some(parent_id)
        } else {
            None
        }
    }

    /// Compute completion percentage for the list.
    #[must_use]
    pub fn completion_percentage(&self) -> u8 {
        if self.items.is_empty() {
            return 0;
        }
        let total = self.items.len();
        let completed = self
            .items
            .iter()
            .filter(|item| item.status == TodoStatus::Completed)
            .count();
        let percent = completed.saturating_mul(100);
        let percent = (percent + total / 2) / total;
        u8::try_from(percent).unwrap_or(u8::MAX)
    }

    /// Return the id of the in-progress item, if any.
    #[must_use]
    pub fn in_progress_id(&self) -> Option<u32> {
        self.items
            .iter()
            .find(|item| item.status == TodoStatus::InProgress)
            .map(|item| item.id)
    }

    /// Clear all todo items.
    pub fn clear(&mut self) {
        self.items.clear();
        self.next_id = 1;
        self.degraded_children.clear();
    }

    /// Rebuild the list from a previously persisted [`TodoListSnapshot`]
    /// (e.g. restored from a saved session). Item ids and statuses are
    /// preserved; `next_id` is advanced past the highest restored id.
    pub fn apply_snapshot(&mut self, snap: TodoListSnapshot) {
        self.items = snap.items;
        self.next_id = self
            .items
            .iter()
            .map(|item| item.id)
            .max()
            .map(|max_id| max_id + 1)
            .unwrap_or(1);
        // Degradation edges are not persisted in the snapshot; rebuild them
        // from any `Degraded` items so the bridge stays consistent on restore.
        self.degraded_children.clear();
        for item in &self.items {
            if item.status == TodoStatus::Degraded {
                self.degraded_children.entry(item.id).or_default();
            }
        }
    }

    fn set_single_in_progress(&mut self, allow_id: Option<u32>) {
        for item in &mut self.items {
            if Some(item.id) != allow_id && item.status == TodoStatus::InProgress {
                item.status = TodoStatus::Pending;
            }
        }
    }
}

// === TodoWriteTool - ToolSpec implementation ===

/// Shared reference to a `TodoList` for use across tools
pub type SharedTodoList = Arc<Mutex<TodoList>>;

/// Create a new shared `TodoList`
pub fn new_shared_todo_list() -> SharedTodoList {
    Arc::new(Mutex::new(TodoList::new()))
}

const CANONICAL_WORK_SURFACE: &str = "checklist";
const DURABLE_WORK_OWNER: &str = "fleet_whaleflow_ledger";

/// Tool for writing and updating the todo list
pub struct TodoWriteTool {
    todo_list: SharedTodoList,
}

impl TodoWriteTool {
    pub fn new(todo_list: SharedTodoList) -> Self {
        Self { todo_list }
    }
}

/// Tool for adding a single todo item.
pub struct TodoAddTool {
    todo_list: SharedTodoList,
}

impl TodoAddTool {
    pub fn new(todo_list: SharedTodoList) -> Self {
        Self { todo_list }
    }
}

#[async_trait]
impl ToolSpec for TodoAddTool {
    fn name(&self) -> &'static str {
        "checklist_add"
    }

    fn description(&self) -> &'static str {
        "Add one checklist item on the active thread/task. Durable tasks persist this checklist as subordinate work progress."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "content": {
                    "type": "string",
                    "description": "The task description"
                },
                "status": {
                    "type": "string",
                    "enum": ["pending", "in_progress", "completed"],
                    "description": "Task status (default: pending)"
                },
                "blocked_by": {
                    "type": "array",
                    "items": { "type": "integer" },
                    "description": "Ids of checklist items that must be completed before this one can start. Self-references, unknown ids, and edges that would form a cycle are ignored."
                }
            },
            "required": ["content"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::WritesFiles]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _context: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let content = input
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::invalid_input("Missing 'content'"))?;
        let status = input
            .get("status")
            .and_then(|v| v.as_str())
            .and_then(TodoStatus::from_str)
            .unwrap_or(TodoStatus::Pending);
        let blocked_by: Vec<u32> = input
            .get("blocked_by")
            .and_then(|v| v.as_array())
            .map(|deps| {
                deps.iter()
                    .filter_map(|dep| dep.as_u64().and_then(|dep| u32::try_from(dep).ok()))
                    .collect()
            })
            .unwrap_or_default();

        let mut list = self.todo_list.lock().await;
        let item = list.add_with_dependencies(content.to_string(), status, blocked_by);
        let snapshot = list.snapshot();

        let result = serde_json::to_string_pretty(&snapshot).unwrap_or_else(|_| "{}".to_string());
        let blocking = if item.blocked_by.is_empty() {
            String::new()
        } else {
            format!(", blocked by {:?}", item.blocked_by)
        };
        Ok(ToolResult::success(format!(
            "Added todo #{} ({}{})\n{}",
            item.id,
            item.status.as_str(),
            blocking,
            result
        ))
        .with_metadata(checklist_metadata(&snapshot)))
    }
}

/// Tool for updating a todo item's status.
pub struct TodoUpdateTool {
    todo_list: SharedTodoList,
}

impl TodoUpdateTool {
    pub fn new(todo_list: SharedTodoList) -> Self {
        Self { todo_list }
    }
}

#[async_trait]
impl ToolSpec for TodoUpdateTool {
    fn name(&self) -> &'static str {
        "checklist_update"
    }

    fn description(&self) -> &'static str {
        "Update one checklist item's status by id on the active thread/task."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "integer",
                    "description": "Todo item id"
                },
                "status": {
                    "type": "string",
                    "enum": ["pending", "in_progress", "completed"],
                    "description": "New status"
                }
            },
            "required": ["id", "status"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::WritesFiles]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _context: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let id = input
            .get("id")
            .and_then(|v| v.as_u64())
            .and_then(|v| u32::try_from(v).ok())
            .ok_or_else(|| ToolError::invalid_input("Missing or invalid 'id'"))?;
        let status = input
            .get("status")
            .and_then(|v| v.as_str())
            .and_then(TodoStatus::from_str)
            .ok_or_else(|| ToolError::invalid_input("Missing or invalid 'status'"))?;

        let mut list = self.todo_list.lock().await;
        let updated = match list.try_update_status(id, status) {
            Ok(updated) => updated,
            Err(unmet) => {
                return Ok(ToolResult::error(format!(
                    "Todo #{id} is blocked by unfinished item(s) {unmet:?}; complete them before starting it."
                )));
            }
        };
        let snapshot = list.snapshot();
        let result = serde_json::to_string_pretty(&snapshot).unwrap_or_else(|_| "{}".to_string());

        match updated {
            Some(item) => Ok(ToolResult::success(format!(
                "Updated todo #{} to {}\n{}",
                item.id,
                item.status.as_str(),
                result
            ))
            .with_metadata(checklist_metadata(&snapshot))),
            None => Ok(ToolResult::error(format!("Todo id {id} not found"))),
        }
    }
}

/// Tool for listing current todos.
pub struct TodoListTool {
    todo_list: SharedTodoList,
}

impl TodoListTool {
    pub fn new(todo_list: SharedTodoList) -> Self {
        Self { todo_list }
    }
}

#[async_trait]
impl ToolSpec for TodoListTool {
    fn name(&self) -> &'static str {
        "checklist_list"
    }

    fn description(&self) -> &'static str {
        "List current checklist progress for the active thread/task."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {}
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    async fn execute(
        &self,
        _input: serde_json::Value,
        _context: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let list = self.todo_list.lock().await;
        let snapshot = list.snapshot();
        let result = serde_json::to_string_pretty(&snapshot).unwrap_or_else(|_| "{}".to_string());
        Ok(ToolResult::success(format!(
            "Todo list ({} items, {}% complete)\n{}",
            snapshot.items.len(),
            snapshot.completion_pct,
            result
        ))
        .with_metadata(checklist_metadata(&snapshot)))
    }
}

#[async_trait]
impl ToolSpec for TodoWriteTool {
    fn name(&self) -> &'static str {
        "checklist_write"
    }

    fn description(&self) -> &'static str {
        "Replace the active thread/task checklist. Use this for granular progress under the current durable task or runtime thread; durable tasks remain the real executable work object."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "todos": {
                    "type": "array",
                    "description": "The complete list of todo items. This replaces the existing list.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "content": {
                                "type": "string",
                                "description": "The task description"
                            },
                            "status": {
                                "type": "string",
                                "enum": ["pending", "in_progress", "completed"],
                                "description": "Task status"
                            },
                            "blocked_by": {
                                "type": "array",
                                "items": { "type": "integer" },
                                "description": "Ids of items that must complete first. Items are numbered from 1 in array order, so only earlier positions can be referenced."
                            }
                        },
                        "required": ["content", "status"]
                    }
                }
            },
            "required": ["todos"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::WritesFiles]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _context: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let todos = input
            .get("todos")
            .and_then(|v| v.as_array())
            .ok_or_else(|| ToolError::invalid_input("Missing or invalid 'todos' array"))?;

        let mut list = self.todo_list.lock().await;

        // Clear and rebuild the list
        list.clear();

        for item in todos {
            let content = item
                .get("content")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ToolError::invalid_input("Todo item missing 'content'"))?;

            let status_str = item
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("pending");

            let status = TodoStatus::from_str(status_str).unwrap_or(TodoStatus::Pending);

            let blocked_by: Vec<u32> = item
                .get("blocked_by")
                .and_then(|v| v.as_array())
                .map(|deps| {
                    deps.iter()
                        .filter_map(|dep| dep.as_u64().and_then(|dep| u32::try_from(dep).ok()))
                        .collect()
                })
                .unwrap_or_default();

            list.add_with_dependencies(content.to_string(), status, blocked_by);
        }

        let snapshot = list.snapshot();
        let result = serde_json::to_string_pretty(&snapshot).unwrap_or_else(|_| "{}".to_string());

        Ok(ToolResult::success(format!(
            "Todo list updated ({} items, {}% complete)\n{}",
            snapshot.items.len(),
            snapshot.completion_pct,
            result
        ))
        .with_metadata(checklist_metadata(&snapshot)))
    }
}

fn checklist_metadata(snapshot: &TodoListSnapshot) -> serde_json::Value {
    let items = snapshot
        .items
        .iter()
        .map(|item| {
            json!({
                "id": item.id,
                "content": item.content,
                "status": item.status.as_str(),
                "blocked_by": item.blocked_by,
            })
        })
        .collect::<Vec<_>>();
    json!({
        "work_surface": {
            "canonical": CANONICAL_WORK_SURFACE,
            "model_visible": true,
            "durable_owner": DURABLE_WORK_OWNER,
            "progress_key": "task_updates.checklist"
        },
        "task_updates": {
            "checklist": {
                "items": items,
                "completion_pct": snapshot.completion_pct,
                "in_progress_id": snapshot.in_progress_id,
                "ready_ids": snapshot.ready_ids,
                "blocked_ids": snapshot.blocked_ids,
                "updated_at": null
            }
        }
    })
}

#[cfg(test)]
mod plan_persist_tests {
    use super::*;

    #[test]
    fn todo_list_snapshot_round_trips_via_apply() {
        let mut original = TodoList::new();
        original.add("one".to_string(), TodoStatus::Completed);
        original.add("two".to_string(), TodoStatus::InProgress);
        let snap = original.snapshot();

        // Serialize + deserialize to prove Deserialize is wired up.
        let json = serde_json::to_string(&snap).unwrap();
        let snap: TodoListSnapshot = serde_json::from_str(&json).unwrap();

        let mut restored = TodoList::new();
        restored.apply_snapshot(snap);

        assert_eq!(restored.snapshot().items.len(), 2);
        assert_eq!(restored.snapshot().items[0].content, "one");
        assert_eq!(restored.snapshot().items[0].status, TodoStatus::Completed);
        // next_id must advance past the highest restored id (2 -> 3).
        let mut next = TodoList::new();
        next.apply_snapshot(restored.snapshot());
        let added = next.add("three".to_string(), TodoStatus::Pending);
        assert_eq!(added.id, 3);
    }

    #[test]
    fn empty_todo_snapshot_restores_empty_list() {
        let snap = TodoListSnapshot {
            items: vec![],
            completion_pct: 0,
            in_progress_id: None,
            ready_ids: Vec::new(),
            blocked_ids: Vec::new(),
        };
        let mut list = TodoList::new();
        list.apply_snapshot(snap);
        assert!(list.snapshot().items.is_empty());
        // next_id resets to 1 for an empty restore.
        let added = list.add("x".to_string(), TodoStatus::Pending);
        assert_eq!(added.id, 1);
    }

    /// A checklist saved before `blocked_by` existed must still load.
    #[test]
    fn snapshot_without_blocked_by_still_deserializes() {
        let legacy = r#"{
            "items": [{ "id": 1, "content": "one", "status": "pending" }],
            "completion_pct": 0,
            "in_progress_id": null
        }"#;
        let snap: TodoListSnapshot = serde_json::from_str(legacy).unwrap();
        assert!(snap.items[0].blocked_by.is_empty());
    }
}

#[cfg(test)]
mod dependency_tests {
    use super::*;

    fn list_with_dependency() -> TodoList {
        let mut list = TodoList::new();
        list.add("upstream".to_string(), TodoStatus::Pending);
        list.add_with_dependencies("downstream".to_string(), TodoStatus::Pending, vec![1]);
        list
    }

    #[test]
    fn blocked_item_is_excluded_from_ready_set() {
        let list = list_with_dependency();
        assert_eq!(list.ready_ids(), vec![1]);
        assert_eq!(list.blocked_ids(), vec![2]);
    }

    #[test]
    fn blocked_item_cannot_be_started() {
        let mut list = list_with_dependency();
        assert_eq!(
            list.try_update_status(2, TodoStatus::InProgress)
                .unwrap_err(),
            vec![1]
        );
        // The rejected item must not have been mutated.
        assert_eq!(list.snapshot().items[1].status, TodoStatus::Pending);
    }

    #[test]
    fn completing_upstream_unblocks_downstream() {
        let mut list = list_with_dependency();
        list.update_status(1, TodoStatus::Completed);
        assert_eq!(list.ready_ids(), vec![2]);
        assert!(list.blocked_ids().is_empty());
        assert!(list.try_update_status(2, TodoStatus::InProgress).is_ok());
    }

    #[test]
    fn self_reference_and_unknown_ids_are_dropped() {
        let mut list = TodoList::new();
        let item = list.add_with_dependencies("solo".to_string(), TodoStatus::Pending, vec![1, 99]);
        assert!(item.blocked_by.is_empty());
        assert_eq!(list.ready_ids(), vec![1]);
    }

    #[test]
    fn cycle_closing_edge_is_rejected() {
        let mut list = TodoList::new();
        list.add("a".to_string(), TodoStatus::Pending);
        list.add_with_dependencies("b".to_string(), TodoStatus::Pending, vec![1]);
        // 1 already depends on nothing; make 3 depend on 2, then confirm a
        // would-be cycle back onto 3 is refused rather than deadlocking.
        let c = list.add_with_dependencies("c".to_string(), TodoStatus::Pending, vec![2]);
        assert_eq!(c.blocked_by, vec![2]);
        // Adding 1→3 would close the loop 1→3→2→1; the cycle-closing edge
        // must be refused, so the would-be dependency is dropped and empty.
        let cyclic = list.sanitize_dependencies(1, vec![3]);
        assert_eq!(cyclic, Vec::<u32>::new());
        // Completing the chain in order must fully drain the blocked set.
        list.update_status(1, TodoStatus::Completed);
        list.update_status(2, TodoStatus::Completed);
        assert_eq!(list.ready_ids(), vec![3]);
    }

    #[test]
    fn deleting_upstream_does_not_strand_dependents() {
        let mut list = list_with_dependency();
        list.clear();
        list.add_with_dependencies("orphan".to_string(), TodoStatus::Pending, vec![42]);
        // Dependency ids that no longer exist count as satisfied.
        assert_eq!(list.ready_ids(), vec![1]);
    }

    #[test]
    fn degrade_to_subtask_regenerates_children_and_marks_parent() {
        let mut list = TodoList::new();
        let failed = list.add("big task".to_string(), TodoStatus::InProgress);
        assert_eq!(failed.id, 1);

        let children = list
            .degrade_to_subtask(
                1,
                vec![
                    "subtask a".to_string(),
                    "subtask b".to_string(),
                    "subtask c".to_string(),
                ],
                None,
                None,
            )
            .expect("degrade should succeed for an existing task");

        // Subtasks were added and are ready to schedule.
        assert_eq!(children.len(), 3);
        assert_eq!(list.ready_ids(), children);
        // Original task is no longer schedulable in either set.
        assert!(!list.ready_ids().contains(&1));
        assert!(!list.blocked_ids().contains(&1));
        // It is marked degraded and tracked.
        assert_eq!(list.get(1).unwrap().status, TodoStatus::Degraded);
        assert!(list.is_degraded(1));
        assert_eq!(list.degraded_subtasks(1), Some(&children));

        // Completing all subtasks resolves the parent.
        for cid in children {
            list.update_status(cid, TodoStatus::Completed);
        }
        assert_eq!(list.try_resolve_degraded(1), Some(1));
        assert_eq!(list.get(1).unwrap().status, TodoStatus::Completed);
        assert!(!list.is_degraded(1));
    }

    #[test]
    fn degrade_rejects_empty_subtasks_and_unknown_id() {
        let mut list = TodoList::new();
        list.add("task".to_string(), TodoStatus::Pending);
        assert!(list
            .degrade_to_subtask(1, vec![], None, None)
            .is_none());
        assert!(list.degrade_to_subtask(99, vec!["x".to_string()], None, None).is_none());
    }

    #[test]
    fn double_degrade_is_idempotent_and_safe() {
        let mut list = TodoList::new();
        list.add("task".to_string(), TodoStatus::Pending);
        let first = list
            .degrade_to_subtask(1, vec!["a".to_string()], None, None)
            .unwrap();
        // Degrading an already-degraded task yields nothing (no duplicate work).
        assert!(list.degrade_to_subtask(1, vec!["b".to_string()], None, None).is_none());
        assert_eq!(list.degraded_subtasks(1), Some(&first));
    }
}

// === Cross-process atomic claim ===
//
// The in-memory `TodoList` is session-scoped: two agents in *different*
// processes (or different sub-agent runtimes sharing a `task_data_dir`) cannot
// coordinate over it. `TodoClaimStore` persists claim ownership to
// `todo_claims.json` under `task_data_dir` and guards every read-modify-write
// with a process-wide advisory file lock (`fd_lock`), so racing `claim` calls
// for the same task id resolve atomically — exactly one agent wins.
//
// `claim` / `reserve` / `take` are three spellings of the same atomic acquire;
// they are kept distinct only so callers can express intent. `release` frees a
// claim, but only the owning agent may do so.

/// A single persisted claim: which agent owns `task_id` and when it grabbed it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClaimRecord {
    pub agent_id: String,
    #[serde(default)]
    pub claimed_at: String,
}

/// On-disk shape of the claim registry.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct TodoClaimsFile {
    claims: HashMap<u32, ClaimRecord>,
}

/// Process-crossing claim coordinator backed by a file + advisory lock.
///
/// Cloning is cheap (just the directory path); the lock and JSON file are
/// opened lazily on each operation, so two clones of the same store share the
/// same on-disk state.
#[derive(Debug, Clone)]
pub struct TodoClaimStore {
    dir: PathBuf,
}

impl TodoClaimStore {
    /// Build a store rooted at `dir`. The directory is created lazily on first
    /// write; a missing directory is not an error until an operation needs it.
    #[must_use]
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// Store path for the claim registry.
    fn claims_path(&self) -> PathBuf {
        let mut p = self.dir.clone();
        p.push("todo_claims.json");
        p
    }

    /// Advisory lock file guarding the claim registry.
    fn lock_path(&self) -> PathBuf {
        let mut p = self.dir.clone();
        p.push("todo_claims.lock");
        p
    }

    /// Run `op` under the process-wide write lock, with the current claim map
    /// loaded into memory. `op` may mutate the map; its final state is written
    /// back to disk before the lock is released. The lock is a blocking
    /// advisory lock, so callers should invoke this from `spawn_blocking`.
    fn with_locked_claims<F, R>(&self, op: F) -> std::io::Result<R>
    where
        F: FnOnce(&mut HashMap<u32, ClaimRecord>) -> R,
    {
        let lock_path = self.lock_path();
        if let Some(parent) = lock_path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        let file = fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)?;
        let mut file_lock = RwLock::new(file);
        let mut guard = file_lock
            .write()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

        let mut claims: HashMap<u32, ClaimRecord> = {
            let path = self.claims_path();
            if path.exists() {
                let raw = fs::read_to_string(&path)?;
                if raw.trim().is_empty() {
                    HashMap::new()
                } else {
                    match serde_json::from_str::<TodoClaimsFile>(&raw) {
                        Ok(f) => f.claims,
                        // A corrupt registry should not wedge every claim; start
                        // fresh rather than failing closed.
                        Err(e) => {
                            tracing::warn!(
                                "todo_claims.json at {} corrupted ({}); starting fresh",
                                path.display(),
                                e
                            );
                            HashMap::new()
                        }
                    }
                }
            } else {
                HashMap::new()
            }
        };

        let result = op(&mut claims);

        let serialized = serde_json::to_string_pretty(&TodoClaimsFile {
            claims: claims.clone(),
        })
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        // Truncate then rewrite so stale bytes from a longer previous version
        // do not linger.
        use std::io::{Seek, Write};
        guard.set_len(0)?;
        guard.seek(std::io::SeekFrom::Start(0))?;
        guard.write_all(serialized.as_bytes())?;
        guard.flush()?;

        Ok(result)
    }

    /// Atomically acquire the claim for `task_id` on behalf of `agent_id`.
    ///
    /// Returns `true` if this call won the claim (first acquirer, or the same
    /// agent re-claiming its own task). Returns `false` if another agent
    /// already holds the claim. `reserve` and `take` are aliases with identical
    /// semantics, kept for caller intent.
    pub fn claim(&self, task_id: u32, agent_id: &str) -> std::io::Result<bool> {
        self.with_locked_claims(|claims| match claims.get(&task_id) {
            Some(existing) if existing.agent_id != agent_id => false,
            _ => {
                claims.insert(
                    task_id,
                    ClaimRecord {
                        agent_id: agent_id.to_string(),
                        claimed_at: Utc::now().to_rfc3339(),
                    },
                );
                true
            }
        })
    }

    /// Alias for [`TodoClaimStore::claim`] — express a reservation intent.
    pub fn reserve(&self, task_id: u32, agent_id: &str) -> std::io::Result<bool> {
        self.claim(task_id, agent_id)
    }

    /// Alias for [`TodoClaimStore::claim`] — express a take/own intent.
    pub fn take(&self, task_id: u32, agent_id: &str) -> std::io::Result<bool> {
        self.claim(task_id, agent_id)
    }

    /// Release a claim. Only the owning `agent_id` may release; releasing a
    /// task you do not own (or one not claimed) returns `false`.
    pub fn release(&self, task_id: u32, agent_id: &str) -> std::io::Result<bool> {
        self.with_locked_claims(|claims| match claims.get(&task_id) {
            Some(existing) if existing.agent_id == agent_id => {
                claims.remove(&task_id);
                true
            }
            _ => false,
        })
    }

    /// Current owner of `task_id`, if any (read-only; still lock-guarded).
    pub fn owner_of(&self, task_id: u32) -> std::io::Result<Option<String>> {
        self.with_locked_claims(|claims| claims.get(&task_id).map(|r| r.agent_id.clone()))
    }
}

/// Tool that lets an agent atomically claim a todo item so two agents never
/// work the same task. Backed by [`TodoClaimStore`] for cross-process safety.
pub struct TodoClaimTool {
    todo_list: SharedTodoList,
    /// Optional shared claim store. When `None` (no `task_data_dir`), claims
    /// degrade to an in-memory advisory check on `todo_list` only.
    claim_store: Option<TodoClaimStore>,
}

impl TodoClaimTool {
    pub fn new(todo_list: SharedTodoList, claim_store: Option<TodoClaimStore>) -> Self {
        Self {
            todo_list,
            claim_store,
        }
    }
}

#[async_trait]
impl ToolSpec for TodoClaimTool {
    fn name(&self) -> &'static str {
        "todo_claim"
    }

    fn description(&self) -> &'static str {
        "Atomically claim a todo item so only one agent works it. Use before starting a todo item in a multi-agent setup. Returns whether the claim was acquired; a false result means another agent already owns it. Use `release` to free it when done. `action` may be `claim`, `reserve`, or `take` (all acquire)."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "integer",
                    "description": "Todo item id to claim (the numeric `id` from todo_write/todo_list)."
                },
                "action": {
                    "type": "string",
                    "enum": ["claim", "reserve", "take", "release"],
                    "description": "claim/reserve/take acquire the item; release frees it. Default claim.",
                    "default": "claim"
                }
            },
            "required": ["id"],
            "additionalProperties": false
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        Vec::new()
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        context: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let id = input
            .get("id")
            .and_then(serde_json::Value::as_u64)
            .and_then(|v| u32::try_from(v).ok())
            .ok_or_else(|| ToolError::invalid_input("`id` (a todo item id) is required"))?;
        let action = input
            .get("action")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("claim")
            .to_ascii_lowercase();

        // Validate the id exists (best-effort; skipped for release when the
        // item may already be gone, and for any cross-process list we can't see).
        {
            let list = self.todo_list.lock().await;
            if list.get(id).is_none() && action != "release" {
                return Err(ToolError::invalid_input(format!(
                    "todo item #{id} does not exist"
                )));
            }
        }

        // Resolve a stable agent identity for this process/runtime.
        let agent_id = context
            .runtime
            .active_task_id
            .clone()
            .or_else(|| context.runtime.active_thread_id.clone())
            .unwrap_or_else(|| "agent:unknown".to_string());

        // Resolve the claim store: prefer the injected one; otherwise derive it
        // from the tool context's task_data_dir so the tool works even when no
        // store was threaded through construction.
        let store = self.claim_store.clone().or_else(|| {
            context
                .runtime
                .task_data_dir
                .clone()
                .map(TodoClaimStore::new)
        });

        let result: std::io::Result<bool> = match store {
            Some(store) => {
                let action_for_spawn = action.clone();
                let agent_id_for_spawn = agent_id.clone();
                tokio::task::spawn_blocking(move || match action_for_spawn.as_str() {
                    "release" => store.release(id, &agent_id_for_spawn),
                    _ => store.claim(id, &agent_id_for_spawn),
                })
                .await
                .map_err(|e| ToolError::execution_failed(format!("claim task join: {e}")))?
            }
            None => {
                // No durable store available: degrade to an in-memory advisory
                // check so single-process callers still get a deterministic
                // answer (no cross-process guarantee).
                let mut list = self.todo_list.lock().await;
                match action.as_str() {
                    "release" => Ok(list.release_claim(id, &agent_id)),
                    _ => Ok(list.claim(id, &agent_id)),
                }
            }
        };

        let acquired =
            result.map_err(|e| ToolError::execution_failed(format!("claim failed: {e}")))?;

        let verb = match action.as_str() {
            "release" => "released",
            _ => "claimed",
        };
        Ok(ToolResult::success(format!(
            "todo item #{id} {verb}: {acquired} (agent {agent_id})"
        )))
    }
}

// In-memory advisory claim helpers on `TodoList`, used only when no durable
// store is available (single-process fallback). These satisfy the same
// acquire/release contract as `TodoClaimStore` but are not cross-process safe.
impl TodoList {
    /// Advisory in-memory claim (see [`TodoClaimStore::claim`]).
    pub fn claim(&mut self, id: u32, agent_id: &str) -> bool {
        match self.claims.get(&id) {
            Some(existing) if existing != agent_id => false,
            _ => {
                self.claims.insert(id, agent_id.to_string());
                true
            }
        }
    }

    /// Advisory in-memory release (see [`TodoClaimStore::release`]).
    pub fn release_claim(&mut self, id: u32, agent_id: &str) -> bool {
        match self.claims.get(&id) {
            Some(existing) if existing == agent_id => {
                self.claims.remove(&id);
                true
            }
            _ => false,
        }
    }
}
