//! Task claim mechanism for sub-agent coordination.
//!
//! Prevents duplicate work by ensuring only one sub-agent can claim a task at a time.
//! Reference: loopx typed todos with claims mechanism.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

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
        if let Some(existing) = claims.get(task_id) {
            if existing.status == TaskClaimStatus::Active && !existing.is_expired() {
                return ClaimResult {
                    success: false,
                    claim: None,
                    error: Some(format!(
                        "Task '{}' is already claimed by '{}'. Use release_task first or wait for claim to expire.",
                        task_id, existing.claimant_id
                    )),
                };
            }
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

        for (task_id, claim) in claims.iter_mut() {
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
}
