//! Embedding service using API (OpenAI/DeepSeek)

use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use crate::Result;
use crate::error::MemoryError;

/// Configuration for the embedding service
#[derive(Debug, Clone)]
pub struct EmbeddingConfig {
    /// API base URL (e.g., "https://api.openai.com/v1")
    pub api_base_url: String,
    /// API key
    pub api_key: String,
    /// Model name (e.g., "text-embedding-3-small")
    pub model: String,
    /// Dimension of the embeddings (default: 1536 for text-embedding-3-small)
    pub dimension: usize,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            api_base_url: "https://api.openai.com/v1".to_string(),
            api_key: String::new(),
            model: "text-embedding-3-small".to_string(),
            dimension: 1536,
        }
    }
}

/// Request body for embedding API
#[derive(Debug, Serialize)]
struct EmbeddingRequest {
    input: Vec<String>,
    model: String,
}

/// Response from embedding API
#[derive(Debug, Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
    #[allow(dead_code)]
    model: String,
    usage: EmbeddingUsage,
}

#[derive(Debug, Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
    #[allow(dead_code)]
    index: usize,
}

#[derive(Debug, Deserialize)]
struct EmbeddingUsage {
    #[allow(dead_code)]
    prompt_tokens: usize,
    #[allow(dead_code)]
    total_tokens: usize,
}

/// Embedding service for generating text embeddings via API
pub struct EmbeddingService {
    client: Client,
    config: EmbeddingConfig,
}

impl EmbeddingService {
    /// Create a new embedding service with the given configuration
    pub fn new(config: EmbeddingConfig) -> Result<Self> {
        info!(
            "Initializing embedding service with model: {}",
            config.model
        );

        let client = Client::new();

        Ok(Self { client, config })
    }

    /// Create a new embedding service with default configuration
    pub fn with_defaults() -> Result<Self> {
        Self::new(EmbeddingConfig::default())
    }

    /// Generate embedding for a single text
    pub async fn embed_text(&self, text: &str) -> Result<Vec<f32>> {
        let embeddings = self.embed_batch(&[text.to_string()]).await?;
        embeddings
            .into_iter()
            .next()
            .ok_or_else(|| MemoryError::Embedding("No embedding returned".to_string()))
    }

    /// Generate embeddings for multiple texts
    pub async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        debug!("Embedding {} texts", texts.len());

        let request = EmbeddingRequest {
            input: texts.to_vec(),
            model: self.config.model.clone(),
        };

        let url = format!("{}/embeddings", self.config.api_base_url);

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(MemoryError::ApiError {
                status: status.as_u16(),
                message: error_text,
            });
        }

        let embedding_response: EmbeddingResponse = response.json().await?;

        // Sort by index to ensure correct order
        let mut data = embedding_response.data;
        data.sort_by_key(|d| d.index);

        let embeddings: Vec<Vec<f32>> = data.into_iter().map(|d| d.embedding).collect();

        debug!(
            "Generated {} embeddings, tokens used: {}",
            embeddings.len(),
            embedding_response.usage.total_tokens
        );

        Ok(embeddings)
    }

    /// Get the dimension of the embeddings
    pub fn dimension(&self) -> usize {
        self.config.dimension
    }

    /// Get the model name
    pub fn model_name(&self) -> &str {
        &self.config.model
    }

    /// Get the configuration
    pub fn config(&self) -> &EmbeddingConfig {
        &self.config
    }
}
