//! Observation compression and summarization

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::Result;
use crate::error::MemoryError;
use crate::vector::{Observation, ObservationKind};

/// Compression strategy for observations
#[derive(Debug, Clone)]
pub enum CompressionStrategy {
    /// Keep the original observation
    Keep,
    /// Merge multiple observations into one
    Merge(Vec<i64>),
    /// Generate a summary
    Summarize {
        original_ids: Vec<i64>,
        summary: String,
    },
    /// Discard low-value observations
    Discard,
}

/// Summary of a session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    /// Session identifier
    pub session_id: String,
    /// Session start time (epoch seconds)
    pub start_time: i64,
    /// Session end time (epoch seconds)
    pub end_time: i64,
    /// Total observations in the session
    pub total_observations: usize,
    /// Number of compressed observations
    pub compressed_observations: usize,
    /// Key decisions made during the session
    pub key_decisions: Vec<String>,
    /// Files modified during the session
    pub files_modified: Vec<String>,
    /// Summary of the session
    pub summary: String,
}

/// Observation compressor
pub struct ObservationCompressor {
    /// Minimum number of observations to trigger compression
    pub min_observations: usize,
    /// Maximum age of observations to consider for compression (in seconds)
    pub max_age_seconds: i64,
}

impl Default for ObservationCompressor {
    fn default() -> Self {
        Self {
            min_observations: 100,
            max_age_seconds: 7 * 24 * 60 * 60, // 7 days
        }
    }
}

impl ObservationCompressor {
    /// Create a new compressor with default settings
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a new compressor with custom settings
    pub fn with_settings(min_observations: usize, max_age_seconds: i64) -> Self {
        Self {
            min_observations,
            max_age_seconds,
        }
    }

    /// Analyze observations and decide compression strategies
    pub fn analyze_observations(&self, observations: &[Observation]) -> Vec<CompressionStrategy> {
        if observations.len() < self.min_observations {
            return observations
                .iter()
                .map(|_| CompressionStrategy::Keep)
                .collect();
        }

        let now = Utc::now().timestamp();
        let cutoff = now - self.max_age_seconds;

        // Group observations by kind and time proximity
        let mut groups: Vec<Vec<&Observation>> = Vec::new();
        let mut current_group: Vec<&Observation> = Vec::new();

        for obs in observations {
            if obs.created_at < cutoff {
                // Old observations: try to merge
                if current_group.is_empty() || self.should_merge(&current_group, obs) {
                    current_group.push(obs);
                } else {
                    if current_group.len() > 1 {
                        groups.push(current_group);
                    }
                    current_group = vec![obs];
                }
            } else {
                // Recent observations: keep them
                if !current_group.is_empty() {
                    if current_group.len() > 1 {
                        groups.push(current_group);
                    }
                    current_group = Vec::new();
                }
            }
        }

        if !current_group.is_empty() && current_group.len() > 1 {
            groups.push(current_group);
        }

        // Generate strategies
        let mut strategies = Vec::new();
        let mut merged_ids = Vec::new();

        for group in &groups {
            let ids: Vec<i64> = group.iter().map(|o| o.id).collect();
            let summary = self.generate_merge_summary(group);
            strategies.push(CompressionStrategy::Summarize {
                original_ids: ids.clone(),
                summary,
            });
            merged_ids.extend(ids);
        }

        // Keep non-merged observations
        for obs in observations {
            if !merged_ids.contains(&obs.id) {
                strategies.push(CompressionStrategy::Keep);
            }
        }

        strategies
    }

    /// Check if two observations should be merged
    fn should_merge(&self, group: &[&Observation], obs: &Observation) -> bool {
        if let Some(last) = group.last() {
            // Same kind
            if last.kind != obs.kind {
                return false;
            }

            // Within 1 hour
            if (obs.created_at - last.created_at).abs() > 3600 {
                return false;
            }

            // Similar files (at least one common file)
            let common_files: Vec<&String> = last
                .files_modified
                .iter()
                .filter(|f| obs.files_modified.contains(f))
                .collect();

            if !common_files.is_empty() {
                return true;
            }

            // Similar concepts
            let common_concepts: Vec<&String> = last
                .concepts
                .iter()
                .filter(|c| obs.concepts.contains(c))
                .collect();

            if common_concepts.len() >= 2 {
                return true;
            }
        }

        false
    }

    /// Generate a summary for merged observations
    fn generate_merge_summary(&self, observations: &[&Observation]) -> String {
        let mut summaries = Vec::new();

        for obs in observations {
            let summary = match obs.kind {
                ObservationKind::Bugfix => format!("Fixed: {}", obs.content),
                ObservationKind::Feature => format!("Added: {}", obs.content),
                ObservationKind::Decision => format!("Decided: {}", obs.content),
                ObservationKind::Discovery => format!("Discovered: {}", obs.content),
                ObservationKind::Change => format!("Changed: {}", obs.content),
                ObservationKind::Manual => obs.content.clone(),
            };
            summaries.push(summary);
        }

        summaries.join("; ")
    }

    /// Generate a session summary
    pub fn summarize_session(
        &self,
        session_id: &str,
        observations: &[Observation],
    ) -> Result<SessionSummary> {
        if observations.is_empty() {
            return Err(MemoryError::InvalidConfig(
                "Cannot summarize empty observations".to_string(),
            ));
        }

        let start_time = observations.iter().map(|o| o.created_at).min().unwrap_or(0);
        let end_time = observations.iter().map(|o| o.created_at).max().unwrap_or(0);

        // Extract key decisions
        let key_decisions: Vec<String> = observations
            .iter()
            .filter(|o| o.kind == ObservationKind::Decision)
            .map(|o| o.content.clone())
            .collect();

        // Extract modified files
        let mut files_modified: Vec<String> = observations
            .iter()
            .flat_map(|o| o.files_modified.clone())
            .collect();
        files_modified.sort();
        files_modified.dedup();

        // Generate summary
        let summary = self.generate_session_summary(observations);

        Ok(SessionSummary {
            session_id: session_id.to_string(),
            start_time,
            end_time,
            total_observations: observations.len(),
            compressed_observations: 0,
            key_decisions,
            files_modified,
            summary,
        })
    }

    /// Generate a summary for the session
    fn generate_session_summary(&self, observations: &[Observation]) -> String {
        let bugfix_count = observations
            .iter()
            .filter(|o| o.kind == ObservationKind::Bugfix)
            .count();
        let feature_count = observations
            .iter()
            .filter(|o| o.kind == ObservationKind::Feature)
            .count();
        let decision_count = observations
            .iter()
            .filter(|o| o.kind == ObservationKind::Decision)
            .count();
        let change_count = observations
            .iter()
            .filter(|o| o.kind == ObservationKind::Change)
            .count();

        let mut summary = format!("Session with {} observations: ", observations.len());

        let mut parts = Vec::new();
        if bugfix_count > 0 {
            parts.push(format!("{} bug fixes", bugfix_count));
        }
        if feature_count > 0 {
            parts.push(format!("{} features", feature_count));
        }
        if decision_count > 0 {
            parts.push(format!("{} decisions", decision_count));
        }
        if change_count > 0 {
            parts.push(format!("{} changes", change_count));
        }

        if parts.is_empty() {
            summary.push_str("general work");
        } else {
            summary.push_str(&parts.join(", "));
        }

        summary
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn test_observations() -> Vec<Observation> {
        let now = Utc::now().timestamp();
        vec![
            Observation {
                id: 1,
                content: "Fixed authentication bug".to_string(),
                kind: ObservationKind::Bugfix,
                project: Some("test-project".to_string()),
                files_read: vec!["src/auth.rs".to_string()],
                files_modified: vec!["src/auth.rs".to_string()],
                concepts: vec!["authentication".to_string()],
                created_at: now - 3600,
            },
            Observation {
                id: 2,
                content: "Added login feature".to_string(),
                kind: ObservationKind::Feature,
                project: Some("test-project".to_string()),
                files_read: vec!["src/login.rs".to_string()],
                files_modified: vec!["src/login.rs".to_string()],
                concepts: vec!["authentication".to_string()],
                created_at: now - 1800,
            },
            Observation {
                id: 3,
                content: "Decided to use JWT".to_string(),
                kind: ObservationKind::Decision,
                project: Some("test-project".to_string()),
                files_read: vec![],
                files_modified: vec![],
                concepts: vec!["authentication".to_string(), "jwt".to_string()],
                created_at: now,
            },
        ]
    }

    #[test]
    fn test_compressor_creation() {
        let compressor = ObservationCompressor::new();
        assert_eq!(compressor.min_observations, 100);
        assert_eq!(compressor.max_age_seconds, 7 * 24 * 60 * 60);
    }

    #[test]
    fn test_analyze_observations_keep_all() {
        let compressor = ObservationCompressor::new();
        let observations = test_observations();

        let strategies = compressor.analyze_observations(&observations);
        assert_eq!(strategies.len(), 3);

        for strategy in &strategies {
            assert!(matches!(strategy, CompressionStrategy::Keep));
        }
    }

    #[test]
    fn test_summarize_session() {
        let compressor = ObservationCompressor::new();
        let observations = test_observations();

        let summary = compressor
            .summarize_session("test-session", &observations)
            .unwrap();

        assert_eq!(summary.session_id, "test-session");
        assert_eq!(summary.total_observations, 3);
        assert_eq!(summary.key_decisions.len(), 1);
        assert!(!summary.summary.is_empty());
    }
}
