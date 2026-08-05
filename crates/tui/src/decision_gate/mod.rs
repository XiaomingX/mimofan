//! Decision gate mechanism for user intervention.
//!
//! Forces user intervention at critical decision points to prevent goal drift
//! during long autonomous runs.
//! Reference: loopx concrete user gates mechanism.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

/// Types of decision gates.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum GateType {
    /// Critical decision that requires user input.
    CriticalDecision,
    /// Permission request for sensitive operations.
    PermissionRequest,
    /// Ambiguity resolution when multiple options exist.
    AmbiguityResolution,
    /// Resource allocation decision.
    ResourceAllocation,
    /// Other gate types.
    Other(String),
}

/// A decision option presented to the user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionOption {
    /// Option identifier.
    pub id: String,
    /// Human-readable label.
    pub label: String,
    /// Detailed description.
    pub description: String,
    /// Whether this is the recommended option.
    pub recommended: bool,
}

/// Status of a decision gate.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum GateStatus {
    /// Waiting for user decision.
    Pending,
    /// User has made a decision.
    Resolved,
    /// Gate has expired.
    Expired,
    /// Gate was cancelled.
    Cancelled,
}

/// A decision gate record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionGate {
    /// Unique gate identifier.
    pub gate_id: String,
    /// Type of gate.
    pub gate_type: GateType,
    /// Description of the decision needed.
    pub description: String,
    /// Available options.
    pub options: Vec<DecisionOption>,
    /// When the gate was created.
    pub created_at: SystemTime,
    /// Optional timeout for the gate.
    pub timeout: Option<Duration>,
    /// Current status.
    pub status: GateStatus,
    /// User's decision (if resolved).
    pub decision: Option<String>,
    /// When the decision was made.
    pub resolved_at: Option<SystemTime>,
    /// Thread/session context.
    pub thread_id: Option<String>,
    /// Goal context.
    pub goal_id: Option<String>,
    /// Additional metadata.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, String>,
}

impl DecisionGate {
    /// Check if the gate has expired.
    pub fn is_expired(&self) -> bool {
        if let Some(timeout) = self.timeout {
            self.status == GateStatus::Pending
                && self
                    .created_at
                    .elapsed()
                    .map(|e| e > timeout)
                    .unwrap_or(false)
        } else {
            false
        }
    }
}

/// Result of a gate operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateResult {
    /// Whether the operation was successful.
    pub success: bool,
    /// The gate record if applicable.
    pub gate: Option<DecisionGate>,
    /// Error message if failed.
    pub error: Option<String>,
}

/// Decision gate manager for user intervention.
pub struct DecisionGateManager {
    /// All gates indexed by gate_id.
    gates: Arc<RwLock<HashMap<String, DecisionGate>>>,
    /// Pending gates (quick lookup).
    pending: Arc<RwLock<Vec<String>>>,
    /// Default timeout for gates.
    default_timeout: Option<Duration>,
}

impl Default for DecisionGateManager {
    fn default() -> Self {
        Self::new(Some(Duration::from_secs(300))) // 5 minutes default
    }
}

impl DecisionGateManager {
    /// Create a new decision gate manager.
    pub fn new(default_timeout: Option<Duration>) -> Self {
        Self {
            gates: Arc::new(RwLock::new(HashMap::new())),
            pending: Arc::new(RwLock::new(Vec::new())),
            default_timeout,
        }
    }

    /// Generate a unique gate ID.
    fn generate_gate_id() -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        format!("gate-{}", timestamp)
    }

    /// Create a new decision gate.
    pub async fn create_gate(
        &self,
        gate_type: GateType,
        description: String,
        options: Vec<DecisionOption>,
        timeout: Option<Duration>,
        thread_id: Option<String>,
        goal_id: Option<String>,
        metadata: HashMap<String, String>,
    ) -> GateResult {
        let gate_id = Self::generate_gate_id();
        let gate = DecisionGate {
            gate_id: gate_id.clone(),
            gate_type,
            description,
            options,
            created_at: SystemTime::now(),
            timeout: timeout.or(self.default_timeout),
            status: GateStatus::Pending,
            decision: None,
            resolved_at: None,
            thread_id,
            goal_id,
            metadata,
        };

        let mut gates = self.gates.write().await;
        let mut pending = self.pending.write().await;

        gates.insert(gate_id.clone(), gate.clone());
        pending.push(gate_id);

        GateResult {
            success: true,
            gate: Some(gate),
            error: None,
        }
    }

    /// Resolve a decision gate with user's choice.
    pub async fn resolve_gate(&self, gate_id: &str, decision: &str) -> GateResult {
        let mut gates = self.gates.write().await;
        let mut pending = self.pending.write().await;

        match gates.get_mut(gate_id) {
            None => GateResult {
                success: false,
                gate: None,
                error: Some(format!("Gate '{}' not found.", gate_id)),
            },
            Some(gate) => {
                if gate.status != GateStatus::Pending {
                    return GateResult {
                        success: false,
                        gate: None,
                        error: Some(format!(
                            "Gate '{}' is not pending (status: {:?}).",
                            gate_id, gate.status
                        )),
                    };
                }

                // Validate decision is a valid option
                if !gate.options.iter().any(|o| o.id == decision) {
                    return GateResult {
                        success: false,
                        gate: None,
                        error: Some(format!(
                            "Decision '{}' is not a valid option for gate '{}'.",
                            decision, gate_id
                        )),
                    };
                }

                gate.status = GateStatus::Resolved;
                gate.decision = Some(decision.to_string());
                gate.resolved_at = Some(SystemTime::now());

                pending.retain(|id| id != gate_id);

                GateResult {
                    success: true,
                    gate: Some(gate.clone()),
                    error: None,
                }
            }
        }
    }

    /// Cancel a decision gate.
    pub async fn cancel_gate(&self, gate_id: &str) -> GateResult {
        let mut gates = self.gates.write().await;
        let mut pending = self.pending.write().await;

        match gates.get_mut(gate_id) {
            None => GateResult {
                success: false,
                gate: None,
                error: Some(format!("Gate '{}' not found.", gate_id)),
            },
            Some(gate) => {
                if gate.status != GateStatus::Pending {
                    return GateResult {
                        success: false,
                        gate: None,
                        error: Some(format!(
                            "Gate '{}' is not pending (status: {:?}).",
                            gate_id, gate.status
                        )),
                    };
                }

                gate.status = GateStatus::Cancelled;
                pending.retain(|id| id != gate_id);

                GateResult {
                    success: true,
                    gate: Some(gate.clone()),
                    error: None,
                }
            }
        }
    }

    /// Get a gate by ID.
    pub async fn get_gate(&self, gate_id: &str) -> Option<DecisionGate> {
        let gates = self.gates.read().await;
        gates.get(gate_id).cloned()
    }

    /// Get all pending gates.
    pub async fn get_pending_gates(&self) -> Vec<DecisionGate> {
        let gates = self.gates.read().await;
        let pending = self.pending.read().await;

        pending
            .iter()
            .filter_map(|id| gates.get(id).cloned())
            .collect()
    }

    /// Get all gates for a thread.
    pub async fn get_thread_gates(&self, thread_id: &str) -> Vec<DecisionGate> {
        let gates = self.gates.read().await;
        gates
            .values()
            .filter(|g| g.thread_id.as_deref() == Some(thread_id))
            .cloned()
            .collect()
    }

    /// Get all gates for a goal.
    pub async fn get_goal_gates(&self, goal_id: &str) -> Vec<DecisionGate> {
        let gates = self.gates.read().await;
        gates
            .values()
            .filter(|g| g.goal_id.as_deref() == Some(goal_id))
            .cloned()
            .collect()
    }

    /// Check and expire any gates that have timed out.
    pub async fn check_expirations(&self) -> Vec<DecisionGate> {
        let mut gates = self.gates.write().await;
        let mut pending = self.pending.write().await;
        let mut expired = Vec::new();

        for gate_id in pending.clone() {
            if let Some(gate) = gates.get_mut(&gate_id)
                && gate.is_expired()
            {
                gate.status = GateStatus::Expired;
                expired.push(gate.clone());
            }
        }

        // Remove expired gates from pending list
        pending.retain(|id| {
            gates
                .get(id)
                .map(|g| g.status == GateStatus::Pending)
                .unwrap_or(false)
        });

        expired
    }

    /// Get gate statistics.
    pub async fn get_stats(&self) -> GateStats {
        let gates = self.gates.read().await;
        let pending = self.pending.read().await;

        GateStats {
            total_gates: gates.len(),
            pending_gates: pending.len(),
            resolved_gates: gates
                .values()
                .filter(|g| g.status == GateStatus::Resolved)
                .count(),
            expired_gates: gates
                .values()
                .filter(|g| g.status == GateStatus::Expired)
                .count(),
            cancelled_gates: gates
                .values()
                .filter(|g| g.status == GateStatus::Cancelled)
                .count(),
        }
    }
}

/// Gate statistics summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateStats {
    pub total_gates: usize,
    pub pending_gates: usize,
    pub resolved_gates: usize,
    pub expired_gates: usize,
    pub cancelled_gates: usize,
}

/// Shared decision gate manager instance.
pub type SharedDecisionGateManager = Arc<DecisionGateManager>;

/// Create a new shared decision gate manager.
pub fn new_shared_decision_gate_manager() -> SharedDecisionGateManager {
    Arc::new(DecisionGateManager::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_gate() {
        let manager = DecisionGateManager::new(None);
        let options = vec![
            DecisionOption {
                id: "yes".to_string(),
                label: "Yes".to_string(),
                description: "Proceed".to_string(),
                recommended: true,
            },
            DecisionOption {
                id: "no".to_string(),
                label: "No".to_string(),
                description: "Cancel".to_string(),
                recommended: false,
            },
        ];

        let result = manager
            .create_gate(
                GateType::CriticalDecision,
                "Test decision".to_string(),
                options,
                None,
                None,
                None,
                HashMap::new(),
            )
            .await;

        assert!(result.success);
        assert!(result.gate.is_some());
        assert_eq!(result.gate.unwrap().status, GateStatus::Pending);
    }

    #[tokio::test]
    async fn test_resolve_gate() {
        let manager = DecisionGateManager::new(None);
        let options = vec![
            DecisionOption {
                id: "yes".to_string(),
                label: "Yes".to_string(),
                description: "Proceed".to_string(),
                recommended: true,
            },
            DecisionOption {
                id: "no".to_string(),
                label: "No".to_string(),
                description: "Cancel".to_string(),
                recommended: false,
            },
        ];

        let result = manager
            .create_gate(
                GateType::CriticalDecision,
                "Test decision".to_string(),
                options,
                None,
                None,
                None,
                HashMap::new(),
            )
            .await;

        let gate_id = result.gate.unwrap().gate_id;

        let result = manager.resolve_gate(&gate_id, "yes").await;
        assert!(result.success);

        let gate = result.gate.unwrap();
        assert_eq!(gate.status, GateStatus::Resolved);
        assert_eq!(gate.decision, Some("yes".to_string()));
    }

    #[tokio::test]
    async fn test_resolve_gate_invalid_option() {
        let manager = DecisionGateManager::new(None);
        let options = vec![DecisionOption {
            id: "yes".to_string(),
            label: "Yes".to_string(),
            description: "Proceed".to_string(),
            recommended: true,
        }];

        let result = manager
            .create_gate(
                GateType::CriticalDecision,
                "Test decision".to_string(),
                options,
                None,
                None,
                None,
                HashMap::new(),
            )
            .await;

        let gate_id = result.gate.unwrap().gate_id;

        let result = manager.resolve_gate(&gate_id, "invalid").await;
        assert!(!result.success);
        assert!(result.error.unwrap().contains("not a valid option"));
    }

    #[tokio::test]
    async fn test_cancel_gate() {
        let manager = DecisionGateManager::new(None);
        let options = vec![DecisionOption {
            id: "yes".to_string(),
            label: "Yes".to_string(),
            description: "Proceed".to_string(),
            recommended: true,
        }];

        let result = manager
            .create_gate(
                GateType::CriticalDecision,
                "Test decision".to_string(),
                options,
                None,
                None,
                None,
                HashMap::new(),
            )
            .await;

        let gate_id = result.gate.unwrap().gate_id;

        let result = manager.cancel_gate(&gate_id).await;
        assert!(result.success);
        assert_eq!(result.gate.unwrap().status, GateStatus::Cancelled);
    }

    #[tokio::test]
    async fn test_get_pending_gates() {
        let manager = DecisionGateManager::new(None);
        let options = vec![DecisionOption {
            id: "yes".to_string(),
            label: "Yes".to_string(),
            description: "Proceed".to_string(),
            recommended: true,
        }];

        manager
            .create_gate(
                GateType::CriticalDecision,
                "Test decision".to_string(),
                options.clone(),
                None,
                None,
                None,
                HashMap::new(),
            )
            .await;

        manager
            .create_gate(
                GateType::PermissionRequest,
                "Permission needed".to_string(),
                options,
                None,
                None,
                None,
                HashMap::new(),
            )
            .await;

        let pending = manager.get_pending_gates().await;
        assert_eq!(pending.len(), 2);
    }

    #[tokio::test]
    async fn test_gate_expiration() {
        let manager = DecisionGateManager::new(Some(Duration::from_millis(10)));
        let options = vec![DecisionOption {
            id: "yes".to_string(),
            label: "Yes".to_string(),
            description: "Proceed".to_string(),
            recommended: true,
        }];

        manager
            .create_gate(
                GateType::CriticalDecision,
                "Test decision".to_string(),
                options,
                None,
                None,
                None,
                HashMap::new(),
            )
            .await;

        tokio::time::sleep(Duration::from_millis(50)).await;

        let expired = manager.check_expirations().await;
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].status, GateStatus::Expired);
    }
}
