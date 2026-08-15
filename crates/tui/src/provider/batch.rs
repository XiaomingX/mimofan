//! Batch API offline channel (#844).
//!
//! Wraps the synchronous LLM client with an async/offline batch submission
//! path so non-interactive work can be collected and submitted to a provider's
//! Batch API endpoint (which typically costs ~50% less than synchronous
//! calls). The plumbing is intentionally provider-agnostic: it talks to an
//! injected [`BatchTransport`] rather than reaching into `ApiClient` internals,
//! so it is fully unit-testable with a [`MockTransport`] and degrades
//! gracefully when the active provider has no batch endpoint.
//!
//! The engine wiring (`EngineConfig::batch_mode`) lives in `engine_config.rs`;
//! this module only provides the client + types + the graceful fallback
//! helper the engine can call later.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::models::{MessageRequest, MessageResponse};

/// A handle identifying a submitted batch on the provider side.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchId(pub String);

impl BatchId {
    /// Generate a fresh random batch id (used by in-memory transports).
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }
}

impl Default for BatchId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for BatchId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Lifecycle status of a submitted batch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BatchStatus {
    /// Batch accepted, not yet started processing.
    Pending,
    /// Batch is actively being processed by the provider.
    InProgress,
    /// Batch completed; `collect` may now be called.
    Completed,
    /// Batch failed on the provider side.
    Failed(String),
}

/// Errors surfaced by the batch client.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum BatchError {
    /// The active provider has no offline batch endpoint; callers should fall
    /// back to synchronous requests.
    #[error("provider does not support batch endpoint: {0}")]
    Unsupported(String),
    /// The batch id is unknown (e.g. double-collected or never submitted).
    #[error("unknown batch id: {0}")]
    UnknownBatch(String),
    /// The batch is not ready to be collected yet.
    #[error("batch {0} is not ready (status: {1:?})")]
    NotReady(String, BatchStatus),
    /// The batch failed on the provider side.
    #[error("batch {0} failed: {1}")]
    Failed(String, String),
}

/// Transport abstraction over a provider's batch endpoint.
///
/// Implementors describe *how* a batch is submitted, polled, and collected. The
/// default [`SyncFallbackTransport`] wraps a synchronous `Fn(MessageRequest)
/// -> Result<MessageResponse, String>` and answers immediately, which is the
/// graceful-degradation path when no async batch endpoint exists. A real
/// provider adapter would instead POST to the `/v1/batches` endpoint and poll
/// an external store.
#[async_trait::async_trait]
pub trait BatchTransport: Send + Sync {
    /// Human-readable name (used for logging / degradation decisions).
    fn name(&self) -> &str;

    /// Whether this transport represents a real offline channel (true) or a
    /// synchronous fallback (false). The client uses this to decide whether a
    /// provider "supports batch".
    fn is_offline(&self) -> bool {
        true
    }

    /// Submit a batch of requests. Returns the provider-side batch id.
    async fn submit(&self, batch: Vec<MessageRequest>) -> Result<BatchId, BatchError>;

    /// Poll the status of a previously submitted batch.
    async fn poll(&self, id: &BatchId) -> Result<BatchStatus, BatchError>;

    /// Collect the responses for a completed batch.
    async fn collect(&self, id: &BatchId) -> Result<Vec<MessageResponse>, BatchError>;
}

/// Synchronous fallback transport.
///
/// Used when the active provider has no asynchronous batch endpoint. Requests
/// are fulfilled inline (one sync call each) so callers get the same
/// submit/poll/collect shape without cost savings. `is_offline` is `false`,
/// signalling the engine to log a degradation notice.
pub struct SyncFallbackTransport<F>
where
    F: Fn(MessageRequest) -> Result<MessageResponse, String> + Send + Sync,
{
    handler: F,
}

impl<F> SyncFallbackTransport<F>
where
    F: Fn(MessageRequest) -> Result<MessageResponse, String> + Send + Sync,
{
    /// Build a fallback transport from a synchronous request handler.
    #[must_use]
    pub fn new(handler: F) -> Self {
        Self { handler }
    }
}

#[async_trait::async_trait]
impl<F> BatchTransport for SyncFallbackTransport<F>
where
    F: Fn(MessageRequest) -> Result<MessageResponse, String> + Send + Sync,
{
    fn name(&self) -> &str {
        "sync-fallback"
    }

    fn is_offline(&self) -> bool {
        false
    }

    async fn submit(&self, batch: Vec<MessageRequest>) -> Result<BatchId, BatchError> {
        // We cannot hold the futures across await without boxing; instead we
        // store the requests in an in-memory pending map keyed by a fresh id
        // and resolve them lazily at `collect`. This keeps the fallback purely
        // synchronous and deterministic.
        let id = BatchId::new();
        FALLBACK_PENDING
            .lock()
            .expect("fallback pending lock poisoned")
            .insert(id.0.clone(), batch);
        Ok(id)
    }

    async fn poll(&self, _id: &BatchId) -> Result<BatchStatus, BatchError> {
        // A sync fallback is always instantly ready.
        Ok(BatchStatus::Completed)
    }

    async fn collect(&self, id: &BatchId) -> Result<Vec<MessageResponse>, BatchError> {
        let pending = FALLBACK_PENDING
            .lock()
            .expect("fallback pending lock poisoned")
            .remove(&id.0);
        let batch = pending.ok_or_else(|| BatchError::UnknownBatch(id.0.clone()))?;
        let mut out = Vec::with_capacity(batch.len());
        for req in batch {
            out.push((self.handler)(req).map_err(|e| BatchError::Failed(id.0.clone(), e))?);
        }
        Ok(out)
    }
}

/// Process-wide map used by the sync fallback to remember pending requests
/// between `submit` and `collect`. Keyed by batch id string.
static FALLBACK_PENDING: std::sync::LazyLock<Mutex<HashMap<String, Vec<MessageRequest>>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

/// The batch client: wraps a [`BatchTransport`] and exposes the stable
/// `submit` / `poll` / `collect` surface the engine routes non-interactive
/// work through.
pub struct BatchClient {
    transport: Arc<dyn BatchTransport>,
}

impl BatchClient {
    /// Build a batch client over a specific transport.
    #[must_use]
    pub fn new(transport: Arc<dyn BatchTransport>) -> Self {
        Self { transport }
    }

    /// Build a client that degrades to synchronous calls via `handler`.
    #[must_use]
    pub fn with_sync_fallback<F>(handler: F) -> Self
    where
        F: Fn(MessageRequest) -> Result<MessageResponse, String> + Send + Sync + 'static,
    {
        Self::new(Arc::new(SyncFallbackTransport::new(handler)))
    }

    /// Whether the underlying transport is a real offline channel.
    #[must_use]
    pub fn is_offline(&self) -> bool {
        self.transport.is_offline()
    }

    /// Transport name (for logging).
    #[must_use]
    pub fn transport_name(&self) -> &str {
        self.transport.name()
    }

    /// Submit a batch of requests.
    pub async fn submit(&self, batch: Vec<MessageRequest>) -> Result<BatchId, BatchError> {
        self.transport.submit(batch).await
    }

    /// Poll a batch's status.
    pub async fn poll(&self, id: &BatchId) -> Result<BatchStatus, BatchError> {
        self.transport.poll(id).await
    }

    /// Collect a batch's responses. Errors if the batch is not yet complete.
    pub async fn collect(&self, id: &BatchId) -> Result<Vec<MessageResponse>, BatchError> {
        // Verify readiness before collecting so callers get a precise error.
        let status = self.transport.poll(id).await?;
        match &status {
            BatchStatus::Completed => self.transport.collect(id).await,
            BatchStatus::Failed(reason) => Err(BatchError::Failed(id.0.clone(), reason.clone())),
            other => Err(BatchError::NotReady(id.0.clone(), other.clone())),
        }
    }
}

/// In-memory mock transport for tests: requests are answered immediately with
/// canned responses, and `poll` can be driven to simulate async progression.
#[cfg(test)]
pub struct MockTransport {
    /// Pre-seeded responses returned in submission order.
    responses: Vec<MessageResponse>,
    /// Simulated delay: number of `poll` calls before `Completed`.
    polls_to_complete: usize,
    /// Per-batch poll counters.
    counters: Mutex<HashMap<String, usize>>,
}

#[cfg(test)]
impl MockTransport {
    /// Build a mock that returns the given responses and becomes ready after
    /// `polls_to_complete` polls.
    #[must_use]
    pub fn new(responses: Vec<MessageResponse>, polls_to_complete: usize) -> Self {
        Self {
            responses,
            polls_to_complete,
            counters: Mutex::new(HashMap::new()),
        }
    }
}

#[cfg(test)]
#[async_trait::async_trait]
impl BatchTransport for MockTransport {
    fn name(&self) -> &str {
        "mock"
    }

    async fn submit(&self, batch: Vec<MessageRequest>) -> Result<BatchId, BatchError> {
        let id = BatchId::new();
        // Stash the request count so collect can return the right slice.
        self.counters
            .lock()
            .expect("mock counter lock poisoned")
            .insert(id.0.clone(), 0);
        // Store requests in the static map under a distinct key to avoid
        // borrow clashes; reuse FALLBACK_PENDING storage pattern via a local.
        MOCK_PENDING
            .lock()
            .expect("mock pending lock poisoned")
            .insert(id.0.clone(), (batch.len(), Vec::new()));
        Ok(id)
    }

    async fn poll(&self, id: &BatchId) -> Result<BatchStatus, BatchError> {
        let mut counters = self.counters.lock().expect("mock counter lock poisoned");
        let entry = counters.get_mut(&id.0).ok_or_else(|| BatchError::UnknownBatch(id.0.clone()))?;
        if *entry >= self.polls_to_complete {
            return Ok(BatchStatus::Completed);
        }
        *entry += 1;
        if *entry >= self.polls_to_complete {
            Ok(BatchStatus::Completed)
        } else {
            Ok(BatchStatus::InProgress)
        }
    }

    async fn collect(&self, id: &BatchId) -> Result<Vec<MessageResponse>, BatchError> {
        let pending = MOCK_PENDING
            .lock()
            .expect("mock pending lock poisoned")
            .get(&id.0)
            .cloned();
        let (count, _) = pending.ok_or_else(|| BatchError::UnknownBatch(id.0.clone()))?;
        let mut out = Vec::with_capacity(count);
        for i in 0..count {
            out.push(
                self.responses
                    .get(i)
                    .cloned()
                    .unwrap_or_else(|| dummy_response(i)),
            );
        }
        Ok(out)
    }
}

#[cfg(test)]
static MOCK_PENDING: std::sync::LazyLock<Mutex<HashMap<String, (usize, Vec<MessageRequest>)>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

#[cfg(test)]
fn dummy_response(i: usize) -> MessageResponse {
    use crate::models::{ContentBlock, Usage};
    MessageResponse {
        id: format!("resp-{i}"),
        r#type: "message".to_string(),
        role: "assistant".to_string(),
        content: vec![ContentBlock::Text {
            text: format!("answer {i}"),
            cache_control: None,
        }],
        model: "mock-model".to_string(),
        stop_reason: Some("end_turn".to_string()),
        stop_sequence: None,
        container: None,
        usage: Usage {
            input_tokens: 1,
            output_tokens: 1,
            ..Default::default()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_request(i: usize) -> MessageRequest {
        use crate::models::{ContentBlock, Message, SystemPrompt};
        MessageRequest {
            model: "mock-model".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: vec![ContentBlock::Text {
                    text: format!("question {i}"),
                    cache_control: None,
                }],
            }],
            max_tokens: 16,
            system: Some(SystemPrompt::Text(format!("sys {i}"))),
            tools: None,
            tool_choice: None,
            metadata: None,
            thinking: None,
            reasoning_effort: None,
            stream: None,
            temperature: None,
            top_p: None,
            response_format: None,
        }
    }

    fn response_for(i: usize) -> MessageResponse {
        use crate::models::{ContentBlock, Usage};
        MessageResponse {
            id: format!("resp-{i}"),
            r#type: "message".to_string(),
            role: "assistant".to_string(),
            content: vec![ContentBlock::Text {
                text: format!("answer {i}"),
                cache_control: None,
            }],
            model: "mock-model".to_string(),
            stop_reason: Some("end_turn".to_string()),
            stop_sequence: None,
            container: None,
            usage: Usage {
                input_tokens: 1,
                output_tokens: 1,
                ..Default::default()
            },
        }
    }

    /// Extract plain text from the first text block of a response.
    fn first_text(resp: &MessageResponse) -> &str {
        for block in &resp.content {
            if let crate::models::ContentBlock::Text { text, .. } = block {
                return text;
            }
        }
        ""
    }

    #[tokio::test]
    async fn mock_batch_round_trip() {
        // Three requests, three responses, ready after 2 polls.
        let responses = vec![response_for(0), response_for(1), response_for(2)];
        let transport = Arc::new(MockTransport::new(responses, 3));
        let client = BatchClient::new(transport);
        assert!(client.is_offline());

        let batch = vec![sample_request(0), sample_request(1), sample_request(2)];
        let id = client.submit(batch).await.expect("submit");

        // Collecting before any poll must error (not ready). `collect`
        // internally re-polls, which advances the mock by one, but with
        // polls_to_complete = 2 it is still short — so it stays NotReady.
        let collected_early = client.collect(&id).await;
        assert!(
            matches!(collected_early, Err(BatchError::NotReady(..))),
            "collect before completion must error"
        );

        // First explicit poll: still in progress.
        let s1 = client.poll(&id).await.unwrap();
        assert!(matches!(s1, BatchStatus::InProgress), "poll 1 should be in progress: {s1:?}");

        // Second poll: completes.
        let s2 = client.poll(&id).await.unwrap();
        assert_eq!(s2, BatchStatus::Completed);

        let out = client.collect(&id).await.expect("collect");
        assert_eq!(out.len(), 3);
        assert_eq!(first_text(&out[0]), "answer 0");
        assert_eq!(first_text(&out[2]), "answer 2");
    }

    #[tokio::test]
    async fn sync_fallback_resolves_immediately() {
        // A provider without a batch endpoint: degrade to sync.
        let client = BatchClient::with_sync_fallback(|req| {
            let idx = req
                .messages
                .first()
                .and_then(|m| m.content.first())
                .map(|b| match b {
                    crate::models::ContentBlock::Text { text, .. } => {
                        text.trim_start_matches("question ").parse::<usize>().unwrap_or(0)
                    }
                    _ => 0,
                })
                .unwrap_or(0);
            Ok(response_for(idx))
        });
        assert!(!client.is_offline(), "sync fallback must report not-offline");
        assert_eq!(client.transport_name(), "sync-fallback");

        let id = client
            .submit(vec![sample_request(1)])
            .await
            .expect("submit");
        // Poll is immediately Completed for the sync fallback.
        assert_eq!(client.poll(&id).await.unwrap(), BatchStatus::Completed);
        let out = client.collect(&id).await.expect("collect");
        assert_eq!(out.len(), 1);
        assert_eq!(first_text(&out[0]), "answer 1");
    }

    #[tokio::test]
    async fn collect_unknown_batch_errors() {
        let client = BatchClient::with_sync_fallback(|_| Ok(response_for(0)));
        let bogus = BatchId("does-not-exist".to_string());
        let res = client.collect(&bogus).await;
        assert!(matches!(res, Err(BatchError::UnknownBatch(_))));
    }
}
