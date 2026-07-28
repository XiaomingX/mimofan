//! External memory service client for long-term memory (#13).
//!
//! Provides integration with external memory services like claude-mem.
//! The service stores and retrieves memories across sessions, enabling
//! the model to remember key information without repeating context.

use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};

/// A single memory entry from the external service.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    /// Unique identifier for this memory.
    pub id: String,
    /// The memory content (text).
    pub content: String,
    /// Optional tags for categorization.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Timestamp when this memory was created.
    #[serde(default)]
    pub created_at: Option<String>,
    /// Timestamp when this memory was last updated.
    #[serde(default)]
    pub updated_at: Option<String>,
    /// Optional relevance score (0.0-1.0) when retrieved by search.
    #[serde(default)]
    pub score: Option<f64>,
}

/// Request to search for memories.
#[derive(Debug, Serialize)]
pub struct MemorySearchRequest {
    /// Search query to find relevant memories.
    pub query: String,
    /// Maximum number of results to return.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
    /// Optional tags to filter by.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

/// Response from memory search.
#[derive(Debug, Deserialize)]
pub struct MemorySearchResponse {
    /// List of matching memories.
    pub memories: Vec<MemoryEntry>,
    /// Total number of memories matching the query.
    #[serde(default)]
    pub total: Option<usize>,
}

/// Request to store a new memory.
#[derive(Debug, Serialize)]
pub struct MemoryStoreRequest {
    /// The memory content to store.
    pub content: String,
    /// Optional tags for categorization.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

/// Client for interacting with an external memory service.
#[derive(Clone)]
pub struct MemoryServiceClient {
    client: Client,
    base_url: String,
    api_key: Option<String>,
}

impl MemoryServiceClient {
    /// Create a new client for the given service URL.
    pub fn new(base_url: &str, api_key: Option<&str>) -> Result<Self> {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .context("Failed to create HTTP client for memory service")?;

        Ok(Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.map(String::from),
        })
    }

    /// Search for memories relevant to the given query.
    pub async fn search_memories(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<MemoryEntry>> {
        let url = format!("{}/v1/memories/search", self.base_url);
        let request = MemorySearchRequest {
            query: query.to_string(),
            limit: Some(limit),
            tags: Vec::new(),
        };

        let mut req = self
            .client
            .post(&url)
            .header("Content-Type", "application/json");

        if let Some(api_key) = &self.api_key {
            req = req.header("Authorization", format!("Bearer {api_key}"));
        }

        let response = req
            .json(&request)
            .send()
            .await
            .context("Failed to send memory search request")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Memory service search failed: {status} - {body}");
        }

        let search_response: MemorySearchResponse = response
            .json()
            .await
            .context("Failed to parse memory search response")?;

        Ok(search_response.memories)
    }

    /// Store a new memory.
    pub async fn store_memory(
        &self,
        content: &str,
        tags: &[String],
    ) -> Result<MemoryEntry> {
        let url = format!("{}/v1/memories", self.base_url);
        let request = MemoryStoreRequest {
            content: content.to_string(),
            tags: tags.to_vec(),
        };

        let mut req = self
            .client
            .post(&url)
            .header("Content-Type", "application/json");

        if let Some(api_key) = &self.api_key {
            req = req.header("Authorization", format!("Bearer {api_key}"));
        }

        let response = req
            .json(&request)
            .send()
            .await
            .context("Failed to send memory store request")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Memory service store failed: {status} - {body}");
        }

        let entry: MemoryEntry = response
            .json()
            .await
            .context("Failed to parse memory store response")?;

        Ok(entry)
    }

    /// Health check for the memory service.
    pub async fn health_check(&self) -> bool {
        let url = format!("{}/health", self.base_url);
        self.client
            .get(&url)
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }
}
