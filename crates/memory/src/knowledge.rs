//! Knowledge agent and corpus functionality

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tracing::info;

use crate::Result;
use crate::embedding::EmbeddingService;
use crate::error::MemoryError;
use crate::vector::{Observation, SearchFilters, VectorMatch, VectorStore};

/// Knowledge corpus
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeCorpus {
    /// Corpus name
    pub name: String,
    /// Corpus description
    pub description: String,
    /// Project this corpus belongs to
    pub project: String,
    /// Number of observations in the corpus
    pub observation_count: usize,
    /// Key concepts in the corpus
    pub concepts: Vec<String>,
    /// Creation timestamp (epoch seconds)
    pub created_at: i64,
    /// Last updated timestamp (epoch seconds)
    pub updated_at: i64,
}

/// Corpus source for answers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorpusSource {
    /// Observation ID
    pub observation_id: i64,
    /// Content snippet
    pub content: String,
    /// Relevance score (0.0 to 1.0)
    pub relevance_score: f64,
    /// Creation timestamp (epoch seconds)
    pub created_at: i64,
}

/// Answer from corpus query
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorpusAnswer {
    /// The answer text
    pub answer: String,
    /// Sources used for the answer
    pub sources: Vec<CorpusSource>,
    /// Confidence score (0.0 to 1.0)
    pub confidence: f64,
}

/// Corpus metadata for storage
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CorpusMetadata {
    /// Corpus name
    name: String,
    /// Corpus description
    description: String,
    /// Project name
    project: String,
    /// Observation IDs in the corpus
    observation_ids: Vec<i64>,
    /// Key concepts
    concepts: Vec<String>,
    /// Creation timestamp
    created_at: i64,
    /// Last updated timestamp
    updated_at: i64,
}

/// Knowledge agent for corpus management and querying
pub struct KnowledgeAgent {
    vector_store: VectorStore,
    embedding_service: EmbeddingService,
    corpora_path: PathBuf,
    corpora: HashMap<String, CorpusMetadata>,
}

impl KnowledgeAgent {
    /// Create a new knowledge agent
    pub fn new(
        vector_store: VectorStore,
        embedding_service: EmbeddingService,
        corpora_path: &Path,
    ) -> Result<Self> {
        info!("Initializing knowledge agent");

        // Create corpora directory if it doesn't exist
        std::fs::create_dir_all(corpora_path)?;

        // Load existing corpora
        let corpora = Self::load_corpora(corpora_path)?;

        Ok(Self {
            vector_store,
            embedding_service,
            corpora_path: corpora_path.to_path_buf(),
            corpora,
        })
    }

    /// Load corpora from disk
    fn load_corpora(path: &Path) -> Result<HashMap<String, CorpusMetadata>> {
        let mut corpora = HashMap::new();

        if !path.exists() {
            return Ok(corpora);
        }

        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                    let data = std::fs::read_to_string(&path)?;
                    let metadata: CorpusMetadata = serde_json::from_str(&data)?;
                    corpora.insert(name.to_string(), metadata);
                }
            }
        }

        Ok(corpora)
    }

    /// Save a corpus to disk
    fn save_corpus(&self, metadata: &CorpusMetadata) -> Result<()> {
        let path = self.corpora_path.join(format!("{}.json", metadata.name));
        let data = serde_json::to_string_pretty(metadata)?;
        std::fs::write(path, data)?;
        Ok(())
    }

    /// Build a corpus from observations matching filters
    pub async fn build_corpus(
        &mut self,
        name: &str,
        description: &str,
        project: &str,
        filters: &SearchFilters,
    ) -> Result<KnowledgeCorpus> {
        info!("Building corpus: {}", name);

        // Search for observations matching filters
        // Use a generic query to get all observations
        let query_embedding = self.embedding_service.embed_text("general").await?;

        let matches = self.vector_store.search(&query_embedding, 1000, filters)?;

        let observation_ids: Vec<i64> = matches.iter().map(|m| m.observation.id).collect();

        // Extract concepts
        let mut concepts: Vec<String> = matches
            .iter()
            .flat_map(|m| m.observation.concepts.clone())
            .collect();
        concepts.sort();
        concepts.dedup();

        let now = chrono::Utc::now().timestamp();

        let metadata = CorpusMetadata {
            name: name.to_string(),
            description: description.to_string(),
            project: project.to_string(),
            observation_ids,
            concepts,
            created_at: now,
            updated_at: now,
        };

        // Save to disk
        self.save_corpus(&metadata)?;

        // Add to in-memory cache
        self.corpora.insert(name.to_string(), metadata.clone());

        let corpus = KnowledgeCorpus {
            name: metadata.name,
            description: metadata.description,
            project: metadata.project,
            observation_count: metadata.observation_ids.len(),
            concepts: metadata.concepts,
            created_at: metadata.created_at,
            updated_at: metadata.updated_at,
        };

        info!(
            "Built corpus with {} observations",
            corpus.observation_count
        );

        Ok(corpus)
    }

    /// Query a corpus with a question
    pub async fn query_corpus(&self, corpus_name: &str, question: &str) -> Result<CorpusAnswer> {
        info!("Querying corpus: {}", corpus_name);

        // Get corpus metadata
        let metadata = self.corpora.get(corpus_name).ok_or_else(|| {
            MemoryError::InvalidConfig(format!("Corpus not found: {}", corpus_name))
        })?;

        // Generate embedding for question
        let question_embedding = self.embedding_service.embed_text(question).await?;

        // Search with project filter
        let filters = SearchFilters {
            project: Some(metadata.project.clone()),
            ..Default::default()
        };

        let matches = self
            .vector_store
            .search(&question_embedding, 10, &filters)?;

        // Generate answer
        let answer = self.generate_answer(question, &matches);

        // Calculate confidence
        let confidence = if matches.is_empty() {
            0.0
        } else {
            matches.iter().map(|m| m.score as f64).sum::<f64>() / matches.len() as f64
        };

        // Build sources
        let sources: Vec<CorpusSource> = matches
            .iter()
            .map(|m| CorpusSource {
                observation_id: m.observation.id,
                content: m.observation.content.clone(),
                relevance_score: m.score as f64,
                created_at: m.observation.created_at,
            })
            .collect();

        Ok(CorpusAnswer {
            answer,
            sources,
            confidence,
        })
    }

    /// Generate an answer from matches
    fn generate_answer(&self, question: &str, matches: &[VectorMatch]) -> String {
        if matches.is_empty() {
            return format!("No relevant information found for: {}", question);
        }

        let mut answer = String::new();
        answer.push_str("Based on past work:\n\n");

        for (i, m) in matches.iter().take(5).enumerate() {
            let obs = &m.observation;
            answer.push_str(&format!(
                "{}. [{}] {} (score: {:.2})\n",
                i + 1,
                obs.kind,
                obs.content,
                m.score
            ));

            if !obs.files_modified.is_empty() {
                answer.push_str(&format!("   Files: {}\n", obs.files_modified.join(", ")));
            }
        }

        answer
    }

    /// List all corpora
    pub fn list_corpora(&self) -> Vec<KnowledgeCorpus> {
        self.corpora
            .values()
            .map(|m| KnowledgeCorpus {
                name: m.name.clone(),
                description: m.description.clone(),
                project: m.project.clone(),
                observation_count: m.observation_ids.len(),
                concepts: m.concepts.clone(),
                created_at: m.created_at,
                updated_at: m.updated_at,
            })
            .collect()
    }

    /// Delete a corpus
    pub fn delete_corpus(&mut self, name: &str) -> Result<()> {
        info!("Deleting corpus: {}", name);

        // Remove from disk
        let path = self.corpora_path.join(format!("{}.json", name));
        if path.exists() {
            std::fs::remove_file(path)?;
        }

        // Remove from cache
        self.corpora.remove(name);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_knowledge_corpus_serialization() {
        let corpus = KnowledgeCorpus {
            name: "test-corpus".to_string(),
            description: "Test corpus".to_string(),
            project: "test-project".to_string(),
            observation_count: 10,
            concepts: vec!["test".to_string()],
            created_at: 1000,
            updated_at: 2000,
        };

        let json = serde_json::to_string(&corpus).unwrap();
        let deserialized: KnowledgeCorpus = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.name, corpus.name);
        assert_eq!(deserialized.observation_count, corpus.observation_count);
    }

    #[test]
    fn test_corpus_answer_serialization() {
        let answer = CorpusAnswer {
            answer: "Test answer".to_string(),
            sources: vec![CorpusSource {
                observation_id: 1,
                content: "Test content".to_string(),
                relevance_score: 0.95,
                created_at: 1000,
            }],
            confidence: 0.95,
        };

        let json = serde_json::to_string(&answer).unwrap();
        let deserialized: CorpusAnswer = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.answer, answer.answer);
        assert_eq!(deserialized.confidence, answer.confidence);
    }
}
