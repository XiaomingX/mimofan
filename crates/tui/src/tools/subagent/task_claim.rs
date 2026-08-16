//! Task claim mechanism for sub-agent coordination.
//!
//! Prevents duplicate work by ensuring only one sub-agent can claim a task at a time.
//! Reference: loopx typed todos with claims mechanism.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

/// Default task claim timeout (5 minutes).
const DEFAULT_CLAIM_TIMEOUT: Duration = Duration::from_secs(300);

/// Status of a task claim.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TaskClaimStatus {
    /// Task is actively being worked on.
    Active,
    /// Task completed successfully.
    Completed,
    /// Task was voluntarily released.
    Released,
    /// Task claim expired due to timeout.
    Expired,
}

/// A task claim record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskClaim {
    /// Unique task identifier.
    pub task_id: String,
    /// ID of the sub-agent that claimed the task.
    pub claimant_id: String,
    /// When the task was claimed.
    pub claimed_at: SystemTime,
    /// Claim timeout duration.
    pub timeout: Duration,
    /// Current status.
    pub status: TaskClaimStatus,
    /// Optional description of the task.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl TaskClaim {
    /// Check if the claim has expired.
    pub fn is_expired(&self) -> bool {
        self.status == TaskClaimStatus::Active
            && self
                .claimed_at
                .elapsed()
                .map(|e| e > self.timeout)
                .unwrap_or(false)
    }
}

/// Result of a claim operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaimResult {
    /// Whether the claim was successful.
    pub success: bool,
    /// The claim record if successful.
    pub claim: Option<TaskClaim>,
    /// Error message if failed.
    pub error: Option<String>,
}

/// Task claim manager for coordinating sub-agent work.
pub struct TaskClaimManager {
    /// Active claims indexed by task_id.
    claims: Arc<RwLock<HashMap<String, TaskClaim>>>,
    /// Default timeout for new claims.
    default_timeout: Duration,
}

impl Default for TaskClaimManager {
    fn default() -> Self {
        Self::new(DEFAULT_CLAIM_TIMEOUT)
    }
}

impl TaskClaimManager {
    /// Create a new task claim manager.
    pub fn new(default_timeout: Duration) -> Self {
        Self {
            claims: Arc::new(RwLock::new(HashMap::new())),
            default_timeout,
        }
    }

    /// Attempt to claim a task.
    ///
    /// Returns success only if:
    /// 1. The task is not currently claimed
    /// 2. The existing claim has expired
    pub async fn claim_task(
        &self,
        task_id: &str,
        claimant_id: &str,
        description: Option<String>,
    ) -> ClaimResult {
        let mut claims = self.claims.write().await;

        // Check for existing active claim
        if let Some(existing) = claims.get(task_id)
            && existing.status == TaskClaimStatus::Active
            && !existing.is_expired()
        {
            return ClaimResult {
                success: false,
                claim: None,
                error: Some(format!(
                    "Task '{}' is already claimed by '{}'. Use release_task first or wait for claim to expire.",
                    task_id, existing.claimant_id
                )),
            };
        }

        // Create new claim
        let claim = TaskClaim {
            task_id: task_id.to_string(),
            claimant_id: claimant_id.to_string(),
            claimed_at: SystemTime::now(),
            timeout: self.default_timeout,
            status: TaskClaimStatus::Active,
            description,
        };

        claims.insert(task_id.to_string(), claim.clone());

        ClaimResult {
            success: true,
            claim: Some(claim),
            error: None,
        }
    }

    /// Release a task claim.
    pub async fn release_task(&self, task_id: &str, claimant_id: &str) -> ClaimResult {
        let mut claims = self.claims.write().await;

        match claims.get(task_id) {
            Some(claim) if claim.claimant_id == claimant_id => {
                let mut released = claim.clone();
                released.status = TaskClaimStatus::Released;
                claims.insert(task_id.to_string(), released.clone());

                ClaimResult {
                    success: true,
                    claim: Some(released),
                    error: None,
                }
            }
            Some(claim) => ClaimResult {
                success: false,
                claim: None,
                error: Some(format!(
                    "Task '{}' is claimed by '{}', not '{}'. Only the claimant can release.",
                    task_id, claim.claimant_id, claimant_id
                )),
            },
            None => ClaimResult {
                success: false,
                claim: None,
                error: Some(format!("Task '{}' is not claimed.", task_id)),
            },
        }
    }

    /// Mark a task as completed.
    pub async fn complete_task(&self, task_id: &str, claimant_id: &str) -> ClaimResult {
        let mut claims = self.claims.write().await;

        match claims.get(task_id) {
            Some(claim) if claim.claimant_id == claimant_id => {
                let mut completed = claim.clone();
                completed.status = TaskClaimStatus::Completed;
                claims.insert(task_id.to_string(), completed.clone());

                ClaimResult {
                    success: true,
                    claim: Some(completed),
                    error: None,
                }
            }
            Some(claim) => ClaimResult {
                success: false,
                claim: None,
                error: Some(format!(
                    "Task '{}' is claimed by '{}', not '{}'. Only the claimant can complete.",
                    task_id, claim.claimant_id, claimant_id
                )),
            },
            None => ClaimResult {
                success: false,
                claim: None,
                error: Some(format!("Task '{}' is not claimed.", task_id)),
            },
        }
    }

    /// Query the status of a task claim.
    pub async fn query_task_status(&self, task_id: &str) -> Option<TaskClaim> {
        let claims = self.claims.read().await;
        claims.get(task_id).cloned()
    }

    /// List all active claims.
    pub async fn list_active_claims(&self) -> Vec<TaskClaim> {
        let claims = self.claims.read().await;
        claims
            .values()
            .filter(|c| c.status == TaskClaimStatus::Active)
            .cloned()
            .collect()
    }

    /// Clean up expired claims.
    pub async fn cleanup_expired(&self) -> Vec<TaskClaim> {
        let mut claims = self.claims.write().await;
        let mut expired = Vec::new();

        for claim in claims.values_mut() {
            if claim.status == TaskClaimStatus::Active && claim.is_expired() {
                claim.status = TaskClaimStatus::Expired;
                expired.push(claim.clone());
            }
        }

        expired
    }
}

/// Shared task claim manager instance.
pub type SharedTaskClaimManager = Arc<TaskClaimManager>;

/// Create a new shared task claim manager.
pub fn new_shared_task_claim_manager() -> SharedTaskClaimManager {
    Arc::new(TaskClaimManager::default())
}

// === File-level claim manager (#842) ===
//
// Prevents two concurrently-running sub-agents from mutating the same file.
// Complements `TaskClaimManager` (task-level) with a file-path-keyed lease
// table. The orchestrator is expected to call `plan_disjoint_file_sets` before
// spawning agents so that each agent's declared file scope is already
// non-overlapping; `claim_file`/`release_file` then enforce that at runtime.

/// A single file lease record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileClaim {
    /// Absolute or workspace-relative file path.
    pub path: String,
    /// ID of the sub-agent that holds the lease.
    pub claimant_id: String,
    /// When the lease was acquired.
    pub claimed_at: SystemTime,
    /// Current status.
    pub status: TaskClaimStatus,
}

/// Result of a file-claim operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileClaimResult {
    /// Whether the claim/release succeeded.
    pub success: bool,
    /// The claim record if successful.
    pub claim: Option<FileClaim>,
    /// Error message if failed.
    pub error: Option<String>,
}

/// File-path-keyed lease manager.
pub struct FileClaimManager {
    claims: Arc<RwLock<HashMap<String, FileClaim>>>,
}

impl Default for FileClaimManager {
    fn default() -> Self {
        Self::new()
    }
}

impl FileClaimManager {
    /// Create a new, empty file claim manager.
    pub fn new() -> Self {
        Self {
            claims: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Try to acquire an exclusive lease on `path` for `claimant_id`.
    /// Fails if the path is already actively leased by a different agent.
    pub async fn claim_file(&self, path: &str, claimant_id: &str) -> FileClaimResult {
        let mut claims = self.claims.write().await;
        if let Some(existing) = claims.get(path)
            && existing.status == TaskClaimStatus::Active
            && existing.claimant_id != claimant_id
        {
            return FileClaimResult {
                success: false,
                claim: None,
                error: Some(format!(
                    "File '{}' is already leased by '{}'.",
                    path, existing.claimant_id
                )),
            };
        }
        let claim = FileClaim {
            path: path.to_string(),
            claimant_id: claimant_id.to_string(),
            claimed_at: SystemTime::now(),
            status: TaskClaimStatus::Active,
        };
        claims.insert(path.to_string(), claim.clone());
        FileClaimResult {
            success: true,
            claim: Some(claim),
            error: None,
        }
    }

    /// Release a single file lease held by `claimant_id`.
    pub async fn release_file(&self, path: &str, claimant_id: &str) -> FileClaimResult {
        let mut claims = self.claims.write().await;
        match claims.get(path) {
            Some(c) if c.claimant_id == claimant_id => {
                let mut released = c.clone();
                released.status = TaskClaimStatus::Released;
                claims.insert(path.to_string(), released.clone());
                FileClaimResult {
                    success: true,
                    claim: Some(released),
                    error: None,
                }
            }
            Some(c) => FileClaimResult {
                success: false,
                claim: None,
                error: Some(format!(
                    "File '{}' is leased by '{}', not '{}'.",
                    path, c.claimant_id, claimant_id
                )),
            },
            None => FileClaimResult {
                success: false,
                claim: None,
                error: Some(format!("File '{}' is not leased.", path)),
            },
        }
    }

    /// Release every active lease held by `claimant_id` (call on terminal state).
    pub async fn release_all_for(&self, claimant_id: &str) {
        let mut claims = self.claims.write().await;
        for claim in claims.values_mut() {
            if claim.claimant_id == claimant_id && claim.status == TaskClaimStatus::Active {
                claim.status = TaskClaimStatus::Released;
            }
        }
    }

    /// Snapshot of currently active file leases.
    pub async fn active_leases(&self) -> Vec<FileClaim> {
        let claims = self.claims.read().await;
        claims
            .values()
            .filter(|c| c.status == TaskClaimStatus::Active)
            .cloned()
            .collect()
    }
}

/// Shared file claim manager instance (#842).
pub type SharedFileClaimManager = Arc<FileClaimManager>;

/// Create a new shared file claim manager.
pub fn new_shared_file_claim_manager() -> SharedFileClaimManager {
    Arc::new(FileClaimManager::new())
}

/// A sub-agent's declared file scope for disjoint planning (#842).
#[derive(Debug, Clone)]
pub struct FileScopeAssignment {
    /// Sub-agent identifier.
    pub agent_id: String,
    /// Workspace-relative file paths this agent intends to touch.
    pub files: Vec<String>,
}

/// Assign each agent a mutually-disjoint subset of its requested files.
///
/// Greedy strategy: process agents in ascending order of scope size so that
/// agents with narrower intents get their files first; any file already taken
/// by an earlier agent is dropped from later agents' assignments. This
/// guarantees the returned map has no file claimed by two agents.
///
/// Returns `agent_id -> files it may exclusively own`.
pub fn plan_disjoint_file_sets(
    assignments: &[FileScopeAssignment],
) -> HashMap<String, Vec<String>> {
    let mut ordered: Vec<&FileScopeAssignment> = assignments.iter().collect();
    ordered.sort_by_key(|a| a.files.len());
    let mut taken: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    for a in ordered {
        let mut owned = Vec::new();
        for f in &a.files {
            if taken.insert(f.clone()) {
                owned.push(f.clone());
            }
        }
        out.insert(a.agent_id.clone(), owned);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_claim_task_success() {
        let manager = TaskClaimManager::new(Duration::from_secs(60));
        let result = manager
            .claim_task("task-1", "agent-1", Some("Test task".to_string()))
            .await;

        assert!(result.success);
        assert!(result.claim.is_some());
        assert_eq!(result.claim.unwrap().claimant_id, "agent-1");
    }

    #[tokio::test]
    async fn test_claim_task_conflict() {
        let manager = TaskClaimManager::new(Duration::from_secs(60));

        // First claim succeeds
        let result1 = manager.claim_task("task-1", "agent-1", None).await;
        assert!(result1.success);

        // Second claim fails
        let result2 = manager.claim_task("task-1", "agent-2", None).await;
        assert!(!result2.success);
        assert!(result2.error.unwrap().contains("already claimed"));
    }

    #[tokio::test]
    async fn test_release_task() {
        let manager = TaskClaimManager::new(Duration::from_secs(60));

        manager.claim_task("task-1", "agent-1", None).await;
        let result = manager.release_task("task-1", "agent-1").await;

        assert!(result.success);
        assert_eq!(result.claim.unwrap().status, TaskClaimStatus::Released);
    }

    #[tokio::test]
    async fn test_complete_task() {
        let manager = TaskClaimManager::new(Duration::from_secs(60));

        manager.claim_task("task-1", "agent-1", None).await;
        let result = manager.complete_task("task-1", "agent-1").await;

        assert!(result.success);
        assert_eq!(result.claim.unwrap().status, TaskClaimStatus::Completed);
    }

    #[tokio::test]
    async fn test_claim_expired() {
        let manager = TaskClaimManager::new(Duration::from_millis(10));

        manager.claim_task("task-1", "agent-1", None).await;
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Should be able to claim after expiration
        let result = manager.claim_task("task-1", "agent-2", None).await;
        assert!(result.success);
    }

    // === File-level claim + disjoint planner (#842) ===

    #[tokio::test]
    async fn test_file_claim_conflict() {
        let mgr = FileClaimManager::new();
        assert!(mgr.claim_file("a.java", "agent-1").await.success);
        let r = mgr.claim_file("a.java", "agent-2").await;
        assert!(!r.success);
        assert!(r.error.unwrap().contains("already leased"));
        // same agent can re-claim
        assert!(mgr.claim_file("a.java", "agent-1").await.success);
    }

    #[tokio::test]
    async fn test_release_all_for() {
        let mgr = FileClaimManager::new();
        mgr.claim_file("a.java", "agent-1").await;
        mgr.claim_file("b.java", "agent-1").await;
        mgr.claim_file("c.java", "agent-2").await;
        mgr.release_all_for("agent-1").await;
        assert_eq!(mgr.active_leases().await.len(), 1);
        // after release, agent-3 can take a.java
        assert!(mgr.claim_file("a.java", "agent-3").await.success);
    }

    #[test]
    fn test_plan_disjoint_file_sets() {
        let assignments = vec![
            FileScopeAssignment {
                agent_id: "wide".into(),
                files: vec!["x.java".into(), "y.java".into(), "z.java".into()],
            },
            FileScopeAssignment {
                agent_id: "narrow".into(),
                files: vec!["y.java".into(), "w.java".into()],
            },
        ];
        let plan = plan_disjoint_file_sets(&assignments);
        // narrow (fewer files) is processed first and keeps its exclusive w.java
        // plus wins the contested y.java; wide falls back to its uncontested x,z.
        assert_eq!(
            plan["narrow"],
            vec!["y.java".to_string(), "w.java".to_string()]
        );
        assert_eq!(
            plan["wide"],
            vec!["x.java".to_string(), "z.java".to_string()]
        );
        // disjointness: no file owned by two agents
        let mut seen = std::collections::HashSet::new();
        for files in plan.values() {
            for f in files {
                assert!(seen.insert(f.clone()), "file {f} claimed twice");
            }
        }
    }

    #[test]
    fn test_shared_file_claim_manager_alias() {
        // #842 integration: the shared alias + constructor must exist and
        // resolve to the same concrete type as the bare manager (used by
        // SubAgentManager.file_claims()). Compile-time + smoke check.
        let mgr: SharedFileClaimManager = new_shared_file_claim_manager();
        let ptr = Arc::as_ptr(&mgr) as usize;
        assert!(ptr != 0);
        // The alias must be the Arc<FileClaimManager> form; this assertion
        // only passes if the type alias resolves correctly.
        fn assert_alias(_: &SharedFileClaimManager) {}
        assert_alias(&mgr);
    }
}
