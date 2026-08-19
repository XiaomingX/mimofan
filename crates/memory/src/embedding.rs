//! Embedding service using API (OpenAI/DeepSeek)
//!
//! When the embedding API is unreachable or unconfigured, the service degrades
//! to a deterministic local hash embedding so that memory writes are never
//! silently dropped (#627). The degraded vector is a weak semantic proxy (it
//! preserves lexical overlap, not true semantics) and is clearly surfaced via
//! a warning and the `degraded_count` counter for observability.

use std::any::Any;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
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

/// Abstraction over a text embedder.
///
/// This trait lets memory consumers stay agnostic to *how* a vector is
/// produced. The default production implementation is [`ApiEmbedder`] (remote
/// OpenAI/DeepSeek-compatible API with a deterministic local-hash degrade
/// path), but a future local/on-device embedder can be slotted in without
/// touching callers (#712). Return types mirror the existing API surface
/// (`Vec<f32>`), so adopting the trait is a pure refactor with zero behavior
/// change for existing callers.
/// Future returned by [`Embedder`] embedding methods.
pub type EmbedFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>>;

pub trait Embedder: Send + Sync {
    /// Embed a single text into a vector.
    fn embed(&self, text: &str) -> EmbedFuture<'_, Vec<f32>>;

    /// Embed a batch of texts. Implementations may batch network calls.
    fn embed_batch(&self, texts: &[String]) -> EmbedFuture<'_, Vec<Vec<f32>>>;

    /// Dimension of the produced vectors.
    fn dim(&self) -> usize;

    /// Human-readable model/backend name (for logging & observability).
    fn model_name(&self) -> &str {
        "unknown"
    }

    /// Downcast support so callers holding `&dyn Embedder` can recover the
    /// concrete type (e.g. to read `ApiEmbedder`'s `EmbeddingConfig`).
    fn as_any(&self) -> &dyn Any;
}

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

/// Production [`Embedder`] backed by a remote OpenAI/DeepSeek-compatible API.
///
/// When the upstream API is unreachable or unconfigured, it degrades to a
/// deterministic local hash vector so that memory writes are never silently
/// dropped (#627). This was previously the body of `EmbeddingService`; it is
/// now an injectable backend behind the [`Embedder`] trait (#712).
pub struct ApiEmbedder {
    client: Client,
    config: EmbeddingConfig,
}

impl ApiEmbedder {
    /// Create a new API-backed embedder with the given configuration
    pub fn new(config: EmbeddingConfig) -> Result<Self> {
        info!("Initializing API embedder with model: {}", config.model);

        let client = Client::new();

        Ok(Self { client, config })
    }

    /// Create a new API-backed embedder with default configuration
    pub fn with_defaults() -> Result<Self> {
        Self::new(EmbeddingConfig::default())
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
        Ok(texts
            .iter()
            .map(|t| hash_embed(t, self.config.dimension))
            .collect())
    }
}

impl Embedder for ApiEmbedder {
    fn embed(&self, text: &str) -> Pin<Box<dyn Future<Output = Result<Vec<f32>>> + Send + '_>> {
        let texts = vec![text.to_string()];
        Box::pin(async move {
            let embeddings = self.embed_batch(&texts).await?;
            embeddings
                .into_iter()
                .next()
                .ok_or_else(|| MemoryError::Embedding("No embedding returned".to_string()))
        })
    }

    fn embed_batch(
        &self,
        texts: &[String],
    ) -> Pin<Box<dyn Future<Output = Result<Vec<Vec<f32>>>> + Send + '_>> {
        // Copy inputs so the returned future borrows only `self` (lifetime
        // `'_`), keeping the trait method dyn-compatible and free of the
        // caller's `texts` lifetime.
        let texts = texts.to_vec();
        Box::pin(async move {
            if texts.is_empty() {
                return Ok(Vec::new());
            }

            debug!("Embedding {} texts", texts.len());

            let request = EmbeddingRequest {
                input: texts.clone(),
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
                return self.degrade(
                    &texts,
                    MemoryError::ApiError {
                        status: status.as_u16(),
                        message: error_text,
                    },
                );
            }

            let embedding_response: EmbeddingResponse = match response.json().await {
                Ok(r) => r,
                Err(e) => return self.degrade(&texts, MemoryError::Embedding(e.to_string())),
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
        })
    }

    fn dim(&self) -> usize {
        self.config.dimension
    }

    fn model_name(&self) -> &str {
        &self.config.model
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Embedding service for generating text embeddings.
///
/// Thin facade over an injectable [`Embedder`] backend (default:
/// [`ApiEmbedder`]). Holds the backend behind `Arc` so the service is
/// `Clone`/`Send + Sync` and cheap to share across the memory pipeline. The
/// public async API (`embed_text`/`embed_batch`) and introspection methods
/// (`dimension`/`model_name`/`config`/`degraded_count`) are unchanged from the
/// pre-trait implementation, so existing callers in `injector.rs` /
/// `knowledge.rs` require no edits (#712).
pub struct EmbeddingService {
    backend: Arc<dyn Embedder>,
}

impl EmbeddingService {
    /// Create a new embedding service with the given configuration.
    /// Constructs the default [`ApiEmbedder`] backend.
    pub fn new(config: EmbeddingConfig) -> Result<Self> {
        Ok(Self {
            backend: Arc::new(ApiEmbedder::new(config)?),
        })
    }

    /// Create a new embedding service with default configuration
    pub fn with_defaults() -> Result<Self> {
        Self::new(EmbeddingConfig::default())
    }

    /// Create a service from an explicit [`Embedder`] backend.
    ///
    /// This is the seam that lets a local/on-device embedder replace the
    /// remote API without changing any caller (#712).
    pub fn with_backend(backend: Arc<dyn Embedder>) -> Self {
        Self { backend }
    }

    /// Generate embedding for a single text
    pub async fn embed_text(&self, text: &str) -> Result<Vec<f32>> {
        self.backend.embed(text).await
    }

    /// Generate embeddings for multiple texts
    pub async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        self.backend.embed_batch(texts).await
    }

    /// Get the dimension of the embeddings
    pub fn dimension(&self) -> usize {
        self.backend.dim()
    }

    /// Get the model name
    pub fn model_name(&self) -> &str {
        self.backend.model_name()
    }

    /// Get the configuration of the underlying API backend.
    ///
    /// Returns `None` for backends that are not the default [`ApiEmbedder`]
    /// (e.g. a local embedder with no API config), so callers can still gate
    /// on `config().is_some()` without assuming a remote endpoint exists.
    pub fn config(&self) -> Option<&EmbeddingConfig> {
        self.backend
            .as_any()
            .downcast_ref::<ApiEmbedder>()
            .map(|a| &a.config)
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
pub fn hash_embed(text: &str, dim: usize) -> Vec<f32> {
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
