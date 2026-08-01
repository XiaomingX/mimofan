//! Cross-session memory injection

use serde::{Deserialize, Serialize};
use tracing::info;

use crate::Result;
use crate::embedding::EmbeddingService;
use crate::vector::{SearchFilters, VectorMatch, VectorStore};

/// Memory injection configuration
#[derive(Debug, Clone)]
pub struct InjectionConfig {
    /// Maximum number of observations to inject
    pub max_observations: usize,
    /// Maximum tokens for injection (estimated)
    pub max_tokens: usize,
    /// Number of context items to include around each observation
    pub context_depth: usize,
    /// Whether to include full observation details
    pub include_full_details: bool,
}

impl Default for InjectionConfig {
    fn default() -> Self {
        Self {
            max_observations: 10,
            max_tokens: 4000,
            context_depth: 3,
            include_full_details: true,
        }
    }
}

/// Memory injection result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryInjection {
    /// Summary of relevant past work
    pub summary: String,
    /// Key decisions from past sessions
    pub key_decisions: Vec<String>,
    /// Recent changes relevant to current context
    pub recent_changes: Vec<String>,
    /// Files that have been modified in related work
    pub files_modified: Vec<String>,
    /// Estimated token count
    pub estimated_tokens: usize,
}

/// Memory injector for cross-session context
pub struct MemoryInjector {
    vector_store: VectorStore,
    embedding_service: EmbeddingService,
    config: InjectionConfig,
}

impl MemoryInjector {
    /// Create a new memory injector
    pub fn new(
        vector_store: VectorStore,
        embedding_service: EmbeddingService,
        config: InjectionConfig,
    ) -> Self {
        Self {
            vector_store,
            embedding_service,
            config,
        }
    }

    /// Create a new memory injector with default configuration
    pub fn with_defaults(vector_store: VectorStore, embedding_service: EmbeddingService) -> Self {
        Self::new(vector_store, embedding_service, InjectionConfig::default())
    }

    /// Generate memory injection for current context
    pub async fn generate_injection(
        &self,
        project: &str,
        current_context: &str,
    ) -> Result<MemoryInjection> {
        info!("Generating memory injection for project: {}", project);

        // Generate embedding for current context
        let query_embedding = self.embedding_service.embed_text(current_context).await?;

        // Search for relevant observations
        let filters = SearchFilters {
            project: Some(project.to_string()),
            ..Default::default()
        };

        let matches =
            self.vector_store
                .search(&query_embedding, self.config.max_observations, &filters)?;

        // Generate injection from matches
        let injection = self.matches_to_injection(&matches)?;

        info!(
            "Generated memory injection with {} observations, ~{} tokens",
            injection.key_decisions.len() + injection.recent_changes.len(),
            injection.estimated_tokens
        );

        Ok(injection)
    }

    /// Generate memory injection for a query
    pub async fn query_memory(&self, query: &str) -> Result<MemoryInjection> {
        info!("Querying memory: {}", query);

        // Generate embedding for query
        let query_embedding = self.embedding_service.embed_text(query).await?;

        // Search without project filter
        let matches = self.vector_store.search(
            &query_embedding,
            self.config.max_observations,
            &SearchFilters::default(),
        )?;

        // Generate injection from matches
        let injection = self.matches_to_injection(&matches)?;

        Ok(injection)
    }

    /// Convert search matches to memory injection
    fn matches_to_injection(&self, matches: &[VectorMatch]) -> Result<MemoryInjection> {
        let mut key_decisions = Vec::new();
        let mut recent_changes = Vec::new();
        let mut files_modified = Vec::new();
        let mut estimated_tokens = 0;

        for m in matches {
            let obs = &m.observation;

            // Add to appropriate category
            match obs.kind {
                crate::vector::ObservationKind::Decision => {
                    key_decisions.push(obs.content.clone());
                    estimated_tokens += self.estimate_tokens(&obs.content);
                }
                crate::vector::ObservationKind::Bugfix
                | crate::vector::ObservationKind::Feature
                | crate::vector::ObservationKind::Change => {
                    recent_changes.push(obs.content.clone());
                    estimated_tokens += self.estimate_tokens(&obs.content);
                }
                _ => {}
            }

            // Collect modified files
            for file in &obs.files_modified {
                if !files_modified.contains(file) {
                    files_modified.push(file.clone());
                }
            }
        }

        // Generate summary
        let summary = self.generate_summary(&key_decisions, &recent_changes);
        estimated_tokens += self.estimate_tokens(&summary);

        // Cap at max tokens
        if estimated_tokens > self.config.max_tokens {
            // Truncate lists to fit
            while estimated_tokens > self.config.max_tokens && !recent_changes.is_empty() {
                let removed = recent_changes.pop().unwrap();
                estimated_tokens -= self.estimate_tokens(&removed);
            }
            while estimated_tokens > self.config.max_tokens && !key_decisions.is_empty() {
                let removed = key_decisions.pop().unwrap();
                estimated_tokens -= self.estimate_tokens(&removed);
            }
        }

        Ok(MemoryInjection {
            summary,
            key_decisions,
            recent_changes,
            files_modified,
            estimated_tokens,
        })
    }

    /// Generate a summary from key decisions and recent changes
    fn generate_summary(&self, key_decisions: &[String], recent_changes: &[String]) -> String {
        let mut parts = Vec::new();

        if !key_decisions.is_empty() {
            parts.push(format!(
                "Made {} key decision{}",
                key_decisions.len(),
                if key_decisions.len() == 1 { "" } else { "s" }
            ));
        }

        if !recent_changes.is_empty() {
            parts.push(format!(
                "Made {} change{}",
                recent_changes.len(),
                if recent_changes.len() == 1 { "" } else { "s" }
            ));
        }

        if parts.is_empty() {
            "No relevant past work found".to_string()
        } else {
            format!("Past work: {}", parts.join(", "))
        }
    }

    /// Estimate token count for text (rough: 1 token ≈ 4 characters)
    fn estimate_tokens(&self, text: &str) -> usize {
        text.len() / 4
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    

    #[test]
    fn test_injection_config_default() {
        let config = InjectionConfig::default();
        assert_eq!(config.max_observations, 10);
        assert_eq!(config.max_tokens, 4000);
        assert_eq!(config.context_depth, 3);
    }

    #[test]
    fn test_memory_injection_serialization() {
        let injection = MemoryInjection {
            summary: "Test summary".to_string(),
            key_decisions: vec!["Decision 1".to_string()],
            recent_changes: vec!["Change 1".to_string()],
            files_modified: vec!["src/main.rs".to_string()],
            estimated_tokens: 100,
        };

        let json = serde_json::to_string(&injection).unwrap();
        let deserialized: MemoryInjection = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.summary, injection.summary);
        assert_eq!(deserialized.key_decisions, injection.key_decisions);
    }
}
