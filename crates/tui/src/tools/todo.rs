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
}

/// Snapshot of a todo list for display or serialization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoListSnapshot {
    pub items: Vec<TodoItem>,
    pub completion_pct: u8,
    pub in_progress_id: Option<u32>,
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
        }
    }

    /// Add a new todo item.
    pub fn add(&mut self, content: String, status: TodoStatus) -> TodoItem {
        let status = match status {
            TodoStatus::InProgress => {
                self.set_single_in_progress(None);
                TodoStatus::InProgress
            }
            other => other,
        };

        let item = TodoItem {
            id: self.next_id,
            content,
            status,
        };
        self.next_id += 1;
        self.items.push(item.clone());
        item
    }

    /// Update an item's status by id.
    pub fn update_status(&mut self, id: u32, status: TodoStatus) -> Option<TodoItem> {
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

        let mut list = self.todo_list.lock().await;
        let item = list.add(content.to_string(), status);
        let snapshot = list.snapshot();

        let result = serde_json::to_string_pretty(&snapshot).unwrap_or_else(|_| "{}".to_string());
        Ok(ToolResult::success(format!(
            "Added todo #{} ({})\n{}",
            item.id,
            item.status.as_str(),
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
        let updated = list.update_status(id, status);
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

            list.add(content.to_string(), status);
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
        };
        let mut list = TodoList::new();
        list.apply_snapshot(snap);
        assert!(list.snapshot().items.is_empty());
        // next_id resets to 1 for an empty restore.
        let added = list.add("x".to_string(), TodoStatus::Pending);
        assert_eq!(added.id, 1);
    }
}
