//! Scheduler hint mechanism for multi-task coordination.
//!
//! Provides coordination for parallel task execution to prevent resource
//! contention and ensure proper task ordering.
//! Reference: loopx scheduler hints mechanism.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

/// Priority levels for scheduler hints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Priority {
    /// Low priority - can be deferred.
    Low = 0,
    /// Normal priority - default.
    Normal = 1,
    /// High priority - should run soon.
    High = 2,
    /// Critical priority - must run immediately.
    Critical = 3,
}

impl Default for Priority {
    fn default() -> Self {
        Self::Normal
    }
}

/// Scheduler hint for task coordination.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulerHint {
    /// Task priority (0-255, higher = more priority).
    pub priority: Priority,
    /// Minimum time between executions.
    pub cadence: Option<Duration>,
    /// Cooldown period after execution.
    pub cooldown: Option<Duration>,
    /// Task IDs that must complete before this task can start.
    pub dependencies: Vec<String>,
    /// Maximum concurrent instances of this task.
    pub max_concurrent: usize,
    /// Optional description.
    pub description: Option<String>,
}

impl Default for SchedulerHint {
    fn default() -> Self {
        Self {
            priority: Priority::Normal,
            cadence: None,
            cooldown: None,
            dependencies: Vec::new(),
            max_concurrent: 1,
            description: None,
        }
    }
}

/// Status of a scheduled task.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TaskStatus {
    /// Task is waiting to run.
    Pending,
    /// Task is currently running.
    Running,
    /// Task completed successfully.
    Completed,
    /// Task failed.
    Failed,
    /// Task is in cooldown period.
    CoolingDown,
    /// Task is blocked by dependencies.
    Blocked,
}

/// A scheduled task record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledTask {
    /// Unique task identifier.
    pub task_id: String,
    /// Scheduler hint for this task.
    pub hint: SchedulerHint,
    /// Current status.
    pub status: TaskStatus,
    /// When the task was created.
    pub created_at: SystemTime,
    /// When the task last ran.
    pub last_run: Option<SystemTime>,
    /// When the task will be ready to run again.
    pub ready_at: Option<SystemTime>,
    /// Number of times this task has run.
    pub run_count: usize,
    /// IDs of tasks that are waiting on this task.
    pub dependents: Vec<String>,
}

/// Result of a scheduler operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulerResult {
    /// Whether the operation was successful.
    pub success: bool,
    /// The task record if applicable.
    pub task: Option<ScheduledTask>,
    /// Error message if failed.
    pub error: Option<String>,
}

/// Scheduler manager for coordinating task execution.
pub struct SchedulerManager {
    /// All registered tasks.
    tasks: Arc<RwLock<HashMap<String, ScheduledTask>>>,
    /// Currently running tasks.
    running: Arc<RwLock<HashSet<String>>>,
    /// Task execution history.
    history: Arc<RwLock<VecDeque<TaskExecution>>>,
    /// Maximum history size.
    max_history: usize,
}

/// Record of a task execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskExecution {
    pub task_id: String,
    pub started_at: SystemTime,
    pub completed_at: Option<SystemTime>,
    pub success: bool,
    pub error: Option<String>,
}

impl Default for SchedulerManager {
    fn default() -> Self {
        Self::new(100)
    }
}

impl SchedulerManager {
    /// Create a new scheduler manager.
    pub fn new(max_history: usize) -> Self {
        Self {
            tasks: Arc::new(RwLock::new(HashMap::new())),
            running: Arc::new(RwLock::new(HashSet::new())),
            history: Arc::new(RwLock::new(VecDeque::new())),
            max_history,
        }
    }

    /// Register a task with scheduler hints.
    pub async fn register_task(&self, task_id: &str, hint: SchedulerHint) -> SchedulerResult {
        let mut tasks = self.tasks.write().await;

        if tasks.contains_key(task_id) {
            return SchedulerResult {
                success: false,
                task: None,
                error: Some(format!("Task '{}' is already registered.", task_id)),
            };
        }

        // Register dependencies
        for dep_id in &hint.dependencies {
            if let Some(dep_task) = tasks.get_mut(dep_id) {
                dep_task.dependents.push(task_id.to_string());
            }
        }

        let task = ScheduledTask {
            task_id: task_id.to_string(),
            hint,
            status: TaskStatus::Pending,
            created_at: SystemTime::now(),
            last_run: None,
            ready_at: None,
            run_count: 0,
            dependents: Vec::new(),
        };

        tasks.insert(task_id.to_string(), task.clone());

        SchedulerResult {
            success: true,
            task: Some(task),
            error: None,
        }
    }

    /// Check if a task is ready to run (internal version with pre-acquired locks).
    fn is_ready_internal(
        task_id: &str,
        tasks: &HashMap<String, ScheduledTask>,
        running: &HashSet<String>,
    ) -> bool {
        match tasks.get(task_id) {
            None => false,
            Some(task) => {
                // Check if already running
                if running.contains(task_id) {
                    return false;
                }

                // Check status
                if task.status != TaskStatus::Pending && task.status != TaskStatus::CoolingDown {
                    return false;
                }

                // Check cooldown
                if let Some(ready_at) = task.ready_at {
                    if SystemTime::now() < ready_at {
                        return false;
                    }
                }

                // Check dependencies
                for dep_id in &task.hint.dependencies {
                    if let Some(dep_task) = tasks.get(dep_id) {
                        if dep_task.status != TaskStatus::Completed {
                            return false;
                        }
                    }
                }

                // Check concurrency limit
                let running_count = running
                    .iter()
                    .filter(|id| {
                        tasks
                            .get(*id)
                            .map(|t| t.hint.dependencies.contains(&task_id.to_string()))
                            .unwrap_or(false)
                    })
                    .count();

                running_count < task.hint.max_concurrent
            }
        }
    }

    /// Check if a task is ready to run.
    pub async fn is_ready(&self, task_id: &str) -> bool {
        let tasks = self.tasks.read().await;
        let running = self.running.read().await;
        Self::is_ready_internal(task_id, &tasks, &running)
    }

    /// Mark a task as running.
    pub async fn start_task(&self, task_id: &str) -> SchedulerResult {
        // First check if task is ready using immutable borrows
        {
            let tasks = self.tasks.read().await;
            let running = self.running.read().await;

            if !Self::is_ready_internal(task_id, &tasks, &running) {
                if !tasks.contains_key(task_id) {
                    return SchedulerResult {
                        success: false,
                        task: None,
                        error: Some(format!("Task '{}' is not registered.", task_id)),
                    };
                }
                return SchedulerResult {
                    success: false,
                    task: None,
                    error: Some(format!("Task '{}' is not ready to run.", task_id)),
                };
            }
        }

        // Now update the task with mutable borrows
        let mut tasks = self.tasks.write().await;
        let mut running = self.running.write().await;

        match tasks.get_mut(task_id) {
            None => SchedulerResult {
                success: false,
                task: None,
                error: Some(format!("Task '{}' is not registered.", task_id)),
            },
            Some(task) => {
                task.status = TaskStatus::Running;
                task.last_run = Some(SystemTime::now());
                task.run_count += 1;
                running.insert(task_id.to_string());

                SchedulerResult {
                    success: true,
                    task: Some(task.clone()),
                    error: None,
                }
            }
        }
    }

    /// Mark a task as completed.
    pub async fn complete_task(
        &self,
        task_id: &str,
        success: bool,
        error: Option<String>,
    ) -> SchedulerResult {
        let mut tasks = self.tasks.write().await;
        let mut running = self.running.write().await;
        let mut history = self.history.write().await;

        running.remove(task_id);

        match tasks.get_mut(task_id) {
            None => SchedulerResult {
                success: false,
                task: None,
                error: Some(format!("Task '{}' is not registered.", task_id)),
            },
            Some(task) => {
                task.status = if success {
                    TaskStatus::Completed
                } else {
                    TaskStatus::Failed
                };

                // Set cooldown if specified
                if success {
                    if let Some(cooldown) = task.hint.cooldown {
                        task.ready_at = Some(SystemTime::now() + cooldown);
                        task.status = TaskStatus::CoolingDown;
                    }
                }

                // Record execution
                let execution = TaskExecution {
                    task_id: task_id.to_string(),
                    started_at: task.last_run.unwrap_or_else(SystemTime::now),
                    completed_at: Some(SystemTime::now()),
                    success,
                    error,
                };

                history.push_back(execution);
                if history.len() > self.max_history {
                    history.pop_front();
                }

                SchedulerResult {
                    success: true,
                    task: Some(task.clone()),
                    error: None,
                }
            }
        }
    }

    /// Get all tasks that are ready to run.
    pub async fn get_ready_tasks(&self) -> Vec<ScheduledTask> {
        let tasks = self.tasks.read().await;
        let running = self.running.read().await;
        let mut ready = Vec::new();

        for task_id in tasks.keys() {
            if Self::is_ready_internal(task_id, &tasks, &running) {
                if let Some(task) = tasks.get(task_id) {
                    ready.push(task.clone());
                }
            }
        }

        // Sort by priority
        ready.sort_by(|a, b| b.hint.priority.cmp(&a.hint.priority));
        ready
    }

    /// Get scheduler status.
    pub async fn get_status(&self) -> SchedulerStatus {
        let tasks = self.tasks.read().await;
        let running = self.running.read().await;
        let history = self.history.read().await;

        SchedulerStatus {
            total_tasks: tasks.len(),
            running_tasks: running.len(),
            pending_tasks: tasks
                .values()
                .filter(|t| t.status == TaskStatus::Pending)
                .count(),
            completed_tasks: tasks
                .values()
                .filter(|t| t.status == TaskStatus::Completed)
                .count(),
            failed_tasks: tasks
                .values()
                .filter(|t| t.status == TaskStatus::Failed)
                .count(),
            recent_executions: history.iter().rev().take(10).cloned().collect(),
        }
    }

    /// Remove a task.
    pub async fn remove_task(&self, task_id: &str) -> SchedulerResult {
        let mut tasks = self.tasks.write().await;
        let running = self.running.read().await;

        if running.contains(task_id) {
            return SchedulerResult {
                success: false,
                task: None,
                error: Some(format!("Cannot remove running task '{}'.", task_id)),
            };
        }

        match tasks.remove(task_id) {
            Some(task) => {
                // Remove from dependents lists
                for (_, other_task) in tasks.iter_mut() {
                    other_task.dependents.retain(|id| id != task_id);
                }

                SchedulerResult {
                    success: true,
                    task: Some(task),
                    error: None,
                }
            }
            None => SchedulerResult {
                success: false,
                task: None,
                error: Some(format!("Task '{}' is not registered.", task_id)),
            },
        }
    }
}

/// Scheduler status summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulerStatus {
    pub total_tasks: usize,
    pub running_tasks: usize,
    pub pending_tasks: usize,
    pub completed_tasks: usize,
    pub failed_tasks: usize,
    pub recent_executions: Vec<TaskExecution>,
}

/// Shared scheduler manager instance.
pub type SharedSchedulerManager = Arc<SchedulerManager>;

/// Create a new shared scheduler manager.
pub fn new_shared_scheduler_manager() -> SharedSchedulerManager {
    Arc::new(SchedulerManager::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_register_task() {
        let manager = SchedulerManager::new(10);
        let hint = SchedulerHint::default();

        let result = manager.register_task("task-1", hint).await;
        assert!(result.success);
        assert!(result.task.is_some());
    }

    #[tokio::test]
    async fn test_register_duplicate_task() {
        let manager = SchedulerManager::new(10);
        let hint = SchedulerHint::default();

        manager.register_task("task-1", hint.clone()).await;
        let result = manager.register_task("task-1", hint).await;

        assert!(!result.success);
        assert!(result.error.unwrap().contains("already registered"));
    }

    #[tokio::test]
    async fn test_task_ready() {
        let manager = SchedulerManager::new(10);
        let hint = SchedulerHint::default();

        manager.register_task("task-1", hint).await;
        assert!(manager.is_ready("task-1").await);
    }

    #[tokio::test]
    async fn test_task_not_ready_with_unmet_dependency() {
        let manager = SchedulerManager::new(10);
        let hint = SchedulerHint {
            dependencies: vec!["task-0".to_string()],
            ..Default::default()
        };

        manager
            .register_task("task-0", SchedulerHint::default())
            .await;
        manager.register_task("task-1", hint).await;

        assert!(!manager.is_ready("task-1").await);
    }

    #[tokio::test]
    async fn test_start_and_complete_task() {
        let manager = SchedulerManager::new(10);
        let hint = SchedulerHint::default();

        manager.register_task("task-1", hint).await;

        let result = manager.start_task("task-1").await;
        assert!(result.success);
        assert_eq!(result.task.unwrap().status, TaskStatus::Running);

        let result = manager.complete_task("task-1", true, None).await;
        assert!(result.success);
        assert_eq!(result.task.unwrap().status, TaskStatus::Completed);
    }

    #[tokio::test]
    async fn test_cooldown() {
        let manager = SchedulerManager::new(10);
        let hint = SchedulerHint {
            cooldown: Some(Duration::from_millis(100)),
            ..Default::default()
        };

        manager.register_task("task-1", hint).await;
        manager.start_task("task-1").await;
        manager.complete_task("task-1", true, None).await;

        // Should not be ready immediately after completion
        assert!(!manager.is_ready("task-1").await);

        // Wait for cooldown
        tokio::time::sleep(Duration::from_millis(150)).await;

        // Should be ready after cooldown
        assert!(manager.is_ready("task-1").await);
    }
}
