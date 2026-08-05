//! Evidence logging mechanism for decision auditing.
//!
//! Records structured decision evidence to support drift auditing and
//! traceability. Reference: loopx evidence logging mechanism.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

/// Types of decisions that can be recorded.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DecisionType {
    /// Tool call decision.
    ToolCall,
    /// Goal continuation decision.
    GoalContinuation,
    /// Error recovery decision.
    ErrorRecovery,
    /// User intervention decision.
    UserIntervention,
    /// Task scheduling decision.
    TaskScheduling,
    /// Resource allocation decision.
    ResourceAllocation,
    /// Other decision types.
    Other(String),
}

/// Outcome of a decision.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum EvidenceOutcome {
    /// Decision led to success.
    Success,
    /// Decision led to failure.
    Failure,
    /// Decision was partially successful.
    Partial,
    /// Decision was skipped.
    Skipped,
    /// Decision is pending.
    Pending,
}

/// A single evidence record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionEvidence {
    /// Unique evidence identifier.
    pub id: String,
    /// Thread/session identifier.
    pub thread_id: String,
    /// Turn identifier within the thread.
    pub turn_id: String,
    /// Type of decision.
    pub decision_type: DecisionType,
    /// Context describing the situation.
    pub context: String,
    /// Rationale for the decision.
    pub rationale: String,
    /// Outcome of the decision.
    pub outcome: EvidenceOutcome,
    /// When the evidence was recorded.
    pub timestamp: SystemTime,
    /// Optional metadata.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, String>,
    /// Optional goal identifier for correlation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub goal_id: Option<String>,
}

/// Result of an evidence operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceResult {
    /// Whether the operation was successful.
    pub success: bool,
    /// The evidence record if applicable.
    pub evidence: Option<DecisionEvidence>,
    /// Error message if failed.
    pub error: Option<String>,
}

/// Evidence manager for recording and querying decision evidence.
pub struct EvidenceManager {
    /// All evidence records indexed by id.
    records: Arc<RwLock<HashMap<String, DecisionEvidence>>>,
    /// Evidence records indexed by thread_id for fast lookup.
    by_thread: Arc<RwLock<HashMap<String, Vec<String>>>>,
    /// Evidence records indexed by goal_id for fast lookup.
    by_goal: Arc<RwLock<HashMap<String, Vec<String>>>>,
    /// Maximum number of records to keep.
    max_records: usize,
}

impl Default for EvidenceManager {
    fn default() -> Self {
        Self::new(1000)
    }
}

impl EvidenceManager {
    /// Create a new evidence manager.
    pub fn new(max_records: usize) -> Self {
        Self {
            records: Arc::new(RwLock::new(HashMap::new())),
            by_thread: Arc::new(RwLock::new(HashMap::new())),
            by_goal: Arc::new(RwLock::new(HashMap::new())),
            max_records,
        }
    }

    /// Generate a unique evidence ID.
    fn generate_id() -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        format!("ev-{}", timestamp)
    }

    /// Record a new evidence.
    pub async fn record_evidence(
        &self,
        thread_id: &str,
        turn_id: &str,
        decision_type: DecisionType,
        context: String,
        rationale: String,
        goal_id: Option<String>,
        metadata: HashMap<String, String>,
    ) -> EvidenceResult {
        let id = Self::generate_id();
        let evidence = DecisionEvidence {
            id: id.clone(),
            thread_id: thread_id.to_string(),
            turn_id: turn_id.to_string(),
            decision_type,
            context,
            rationale,
            outcome: EvidenceOutcome::Pending,
            timestamp: SystemTime::now(),
            metadata,
            goal_id,
        };

        let mut records = self.records.write().await;
        let mut by_thread = self.by_thread.write().await;
        let mut by_goal = self.by_goal.write().await;

        // Add to main index
        records.insert(id.clone(), evidence.clone());

        // Add to thread index
        by_thread
            .entry(thread_id.to_string())
            .or_insert_with(Vec::new)
            .push(id.clone());

        // Add to goal index if present
        if let Some(ref goal_id) = evidence.goal_id {
            by_goal
                .entry(goal_id.clone())
                .or_insert_with(Vec::new)
                .push(id.clone());
        }

        // Cleanup old records if needed
        if records.len() > self.max_records {
            let mut sorted_ids: Vec<_> = records.keys().cloned().collect();
            sorted_ids.sort_by_key(|id| {
                records
                    .get(id)
                    .map(|e| e.timestamp)
                    .unwrap_or(SystemTime::UNIX_EPOCH)
            });

            let to_remove = sorted_ids.len() - self.max_records;
            for id in sorted_ids.into_iter().take(to_remove) {
                if let Some(record) = records.remove(&id) {
                    // Remove from thread index
                    if let Some(thread_records) = by_thread.get_mut(&record.thread_id) {
                        thread_records.retain(|r| r != &id);
                    }
                    // Remove from goal index
                    if let Some(ref goal_id) = record.goal_id
                        && let Some(goal_records) = by_goal.get_mut(goal_id)
                    {
                        goal_records.retain(|r| r != &id);
                    }
                }
            }
        }

        EvidenceResult {
            success: true,
            evidence: Some(evidence),
            error: None,
        }
    }

    /// Update the outcome of an evidence record.
    pub async fn update_outcome(
        &self,
        evidence_id: &str,
        outcome: EvidenceOutcome,
    ) -> EvidenceResult {
        let mut records = self.records.write().await;

        match records.get_mut(evidence_id) {
            None => EvidenceResult {
                success: false,
                evidence: None,
                error: Some(format!("Evidence '{}' not found.", evidence_id)),
            },
            Some(evidence) => {
                evidence.outcome = outcome;
                EvidenceResult {
                    success: true,
                    evidence: Some(evidence.clone()),
                    error: None,
                }
            }
        }
    }

    /// Get evidence by ID.
    pub async fn get_evidence(&self, evidence_id: &str) -> Option<DecisionEvidence> {
        let records = self.records.read().await;
        records.get(evidence_id).cloned()
    }

    /// Get all evidence for a thread.
    pub async fn get_thread_evidence(&self, thread_id: &str) -> Vec<DecisionEvidence> {
        let records = self.records.read().await;
        let by_thread = self.by_thread.read().await;

        match by_thread.get(thread_id) {
            None => Vec::new(),
            Some(ids) => ids
                .iter()
                .filter_map(|id| records.get(id).cloned())
                .collect(),
        }
    }

    /// Get all evidence for a goal.
    pub async fn get_goal_evidence(&self, goal_id: &str) -> Vec<DecisionEvidence> {
        let records = self.records.read().await;
        let by_goal = self.by_goal.read().await;

        match by_goal.get(goal_id) {
            None => Vec::new(),
            Some(ids) => ids
                .iter()
                .filter_map(|id| records.get(id).cloned())
                .collect(),
        }
    }

    /// Get recent evidence across all threads.
    pub async fn get_recent_evidence(&self, limit: usize) -> Vec<DecisionEvidence> {
        let records = self.records.read().await;
        let mut recent: Vec<_> = records.values().cloned().collect();

        recent.sort_by_key(|r| std::cmp::Reverse(r.timestamp));
        recent.into_iter().take(limit).collect()
    }

    /// Get evidence by decision type.
    pub async fn get_evidence_by_type(
        &self,
        decision_type: &DecisionType,
    ) -> Vec<DecisionEvidence> {
        let records = self.records.read().await;
        records
            .values()
            .filter(|e| &e.decision_type == decision_type)
            .cloned()
            .collect()
    }

    /// Get evidence by outcome.
    pub async fn get_evidence_by_outcome(
        &self,
        outcome: &EvidenceOutcome,
    ) -> Vec<DecisionEvidence> {
        let records = self.records.read().await;
        records
            .values()
            .filter(|e| &e.outcome == outcome)
            .cloned()
            .collect()
    }

    /// Get evidence count by thread.
    pub async fn get_thread_evidence_count(&self, thread_id: &str) -> usize {
        let by_thread = self.by_thread.read().await;
        by_thread.get(thread_id).map(|v| v.len()).unwrap_or(0)
    }

    /// Get total evidence count.
    pub async fn get_total_count(&self) -> usize {
        let records = self.records.read().await;
        records.len()
    }

    /// Export all evidence as JSON.
    pub async fn export_evidence(&self) -> Vec<DecisionEvidence> {
        let records = self.records.read().await;
        records.values().cloned().collect()
    }
}

/// Shared evidence manager instance.
pub type SharedEvidenceManager = Arc<EvidenceManager>;

/// Create a new shared evidence manager.
pub fn new_shared_evidence_manager() -> SharedEvidenceManager {
    Arc::new(EvidenceManager::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_record_evidence() {
        let manager = EvidenceManager::new(100);
        let mut metadata = HashMap::new();
        metadata.insert("key".to_string(), "value".to_string());

        let result = manager
            .record_evidence(
                "thread-1",
                "turn-1",
                DecisionType::ToolCall,
                "Test context".to_string(),
                "Test rationale".to_string(),
                Some("goal-1".to_string()),
                metadata,
            )
            .await;

        assert!(result.success);
        assert!(result.evidence.is_some());
        assert_eq!(result.evidence.unwrap().thread_id, "thread-1");
    }

    #[tokio::test]
    async fn test_update_outcome() {
        let manager = EvidenceManager::new(100);

        let result = manager
            .record_evidence(
                "thread-1",
                "turn-1",
                DecisionType::ToolCall,
                "Test context".to_string(),
                "Test rationale".to_string(),
                None,
                HashMap::new(),
            )
            .await;

        let evidence_id = result.evidence.unwrap().id;

        let result = manager
            .update_outcome(&evidence_id, EvidenceOutcome::Success)
            .await;

        assert!(result.success);
        assert_eq!(result.evidence.unwrap().outcome, EvidenceOutcome::Success);
    }

    #[tokio::test]
    async fn test_get_thread_evidence() {
        let manager = EvidenceManager::new(100);

        manager
            .record_evidence(
                "thread-1",
                "turn-1",
                DecisionType::ToolCall,
                "Context 1".to_string(),
                "Rationale 1".to_string(),
                None,
                HashMap::new(),
            )
            .await;

        manager
            .record_evidence(
                "thread-1",
                "turn-2",
                DecisionType::GoalContinuation,
                "Context 2".to_string(),
                "Rationale 2".to_string(),
                None,
                HashMap::new(),
            )
            .await;

        manager
            .record_evidence(
                "thread-2",
                "turn-1",
                DecisionType::ToolCall,
                "Context 3".to_string(),
                "Rationale 3".to_string(),
                None,
                HashMap::new(),
            )
            .await;

        let evidence = manager.get_thread_evidence("thread-1").await;
        assert_eq!(evidence.len(), 2);
    }

    #[tokio::test]
    async fn test_get_goal_evidence() {
        let manager = EvidenceManager::new(100);

        manager
            .record_evidence(
                "thread-1",
                "turn-1",
                DecisionType::ToolCall,
                "Context 1".to_string(),
                "Rationale 1".to_string(),
                Some("goal-1".to_string()),
                HashMap::new(),
            )
            .await;

        manager
            .record_evidence(
                "thread-1",
                "turn-2",
                DecisionType::GoalContinuation,
                "Context 2".to_string(),
                "Rationale 2".to_string(),
                Some("goal-1".to_string()),
                HashMap::new(),
            )
            .await;

        manager
            .record_evidence(
                "thread-2",
                "turn-1",
                DecisionType::ToolCall,
                "Context 3".to_string(),
                "Rationale 3".to_string(),
                Some("goal-2".to_string()),
                HashMap::new(),
            )
            .await;

        let evidence = manager.get_goal_evidence("goal-1").await;
        assert_eq!(evidence.len(), 2);
    }

    #[tokio::test]
    async fn test_max_records_limit() {
        let manager = EvidenceManager::new(5);

        for i in 0..10 {
            manager
                .record_evidence(
                    "thread-1",
                    &format!("turn-{}", i),
                    DecisionType::ToolCall,
                    format!("Context {}", i),
                    format!("Rationale {}", i),
                    None,
                    HashMap::new(),
                )
                .await;
        }

        assert_eq!(manager.get_total_count().await, 5);
    }
}
