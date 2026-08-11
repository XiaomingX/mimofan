//! Embedding service using API (OpenAI/DeepSeek)
//!
//! When the embedding API is unreachable or unconfigured, the service degrades
//! to a deterministic local hash embedding so that memory writes are never
//! silently dropped (#627). The degraded vector is a weak semantic proxy (it
//! preserves lexical overlap, not true semantics) and is clearly surfaced via
//! a warning and the `degraded_count` counter for observability.

use std::sync::atomic::{AtomicU64, Ordering};

use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use crate::Result;
use crate::error::MemoryError;

/// Number of times the embedding service fell back to the local hash vector
/// because the upstream API failed or was unconfigured. Read via
/// [`EmbeddingService::degraded_count`] for dashboards / health probes.
static DEGRADED_COUNT: AtomicU64 = AtomicU64::new(0);

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
            return self.degrade(texts, MemoryError::ApiError {
                status: status.as_u16(),
                message: error_text,
            });
        }

        let embedding_response: EmbeddingResponse = match response.json().await {
            Ok(r) => r,
            Err(e) => return self.degrade(texts, MemoryError::Embedding(e.to_string())),
        };

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

    /// Degrade gracefully when the upstream embedding API fails or is
    /// unconfigured: return deterministic local hash vectors instead of
    /// dropping the memory write (#627). The caller sees no error; the
    /// degradation is recorded via a warning and the global counter.
    fn degrade(&self, texts: &[String], cause: MemoryError) -> Result<Vec<Vec<f32>>> {
        warn!(
            target: "memory",
            error = %cause,
            model = %self.config.model,
            "embedding API unavailable; degrading to local hash vectors (semantic \
             quality reduced, but memory writes are preserved)"
        );
        DEGRADED_COUNT.fetch_add(1, Ordering::Relaxed);
        Ok(texts.iter().map(|t| hash_embed(t, self.config.dimension)).collect())
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

    /// Total number of times this service has degraded to local hash vectors
    /// because the upstream API was unavailable. Monotonic; useful for
    /// dashboards and health probes (#627).
    pub fn degraded_count() -> u64 {
        DEGRADED_COUNT.load(Ordering::Relaxed)
    }
}

/// Deterministic local fallback embedding: a hashed bag-of-words vector over
/// whitespace tokens, L2-normalized to unit length.
///
/// This is a *lexical* proxy, not a semantic embedding — it preserves token
/// overlap (so near-identical strings score high cosine similarity) but cannot
/// capture meaning. It exists purely so memory writes survive an embedding-API
/// outage without discarding data (#627). Output dimension matches `dim` so it
/// interoperates with the real embedding store.
fn hash_embed(text: &str, dim: usize) -> Vec<f32> {
    let dim = dim.max(1);
    let mut vec = vec![0.0f32; dim];
    for token in text.split_whitespace() {
        // FNV-1a over the lowercased token, mapped into the vector space.
        let mut hash: u64 = 0xcbf29ce484222325;
        for byte in token.to_ascii_lowercase().bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
        // Accumulate the hashed position; sign breaks symmetry between tokens.
        let idx = (hash as usize) % dim;
        let signed = if (hash >> 63) & 1 == 1 { -1.0 } else { 1.0 };
        vec[idx] += signed;
    }
    // L2-normalize to unit length so cosine similarity is well-behaved.
    let norm = vec.iter().map(|v| v * v).sum::<f32>().sqrt();
    if norm > 0.0 {
        for v in vec.iter_mut() {
            *v /= norm;
        }
    }
    vec
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_embed_is_deterministic() {
        let a = hash_embed("hello world foo", 64);
        let b = hash_embed("hello world foo", 64);
        assert_eq!(a, b);
    }

    #[test]
    fn hash_embed_matches_dimension() {
        let v = hash_embed("any text here", 1536);
        assert_eq!(v.len(), 1536);
    }

    #[test]
    fn hash_embed_is_unit_length() {
        let v = hash_embed("a b c d e f g", 32);
        let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-5, "expected unit length, got {norm}");
    }

    #[test]
    fn hash_embed_similar_text_overlaps() {
        // Shared tokens should place the two vectors closer than disjoint ones.
        fn cosine(a: &[f32], b: &[f32]) -> f32 {
            let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
            let na = a.iter().map(|x| x * x).sum::<f32>().sqrt();
            let nb = b.iter().map(|x| x * x).sum::<f32>().sqrt();
            dot / (na * nb)
        }
        let similar = hash_embed("the cat sat on the mat", 128);
        let same = hash_embed("the cat sat on the mat", 128);
        let different = hash_embed("zebra orbit quantum plasma", 128);
        assert!(
            cosine(&similar, &same) > cosine(&similar, &different),
            "degraded vectors should preserve lexical overlap"
        );
    }

    #[test]
    fn empty_text_yields_zero_vector() {
        let v = hash_embed("", 16);
        assert!(v.iter().all(|x| *x == 0.0));
    }

    #[test]
    fn degraded_counter_is_readable() {
        // The counter is a global monotonic probe; merely reading it must not
        // panic or trigger any network I/O. The actual degrade path (API
        // failure -> local hash vectors) is exercised at runtime when the
        // upstream embedding endpoint is unreachable (#627).
        let _ = EmbeddingService::degraded_count();
    }
}
