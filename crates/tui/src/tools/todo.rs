//! Todo list tool and supporting data structures.

use std::sync::Arc;
use tokio::sync::Mutex;

use async_trait::async_trait;
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
}

impl TodoStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            TodoStatus::Pending => "pending",
            TodoStatus::InProgress => "in_progress",
            TodoStatus::Completed => "completed",
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
}

impl TodoList {
    /// Create an empty todo list.
    #[must_use]
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            next_id: 1,
        }
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
}
