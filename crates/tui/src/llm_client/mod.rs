//! LLM Client Trait and Retry Logic
//!
//! This module provides a unified interface for LLM providers with robust retry logic,
//! exponential backoff, and proper error classification.
//!
//! # Architecture
//!
//! - `LlmClient` trait: Async interface for LLM providers (DeepSeek, `OpenAI`, etc.)
//! - `RetryConfig`: Configurable retry behavior with exponential backoff and jitter
//! - `LlmError`: Classified errors with retryability information

//! - `with_retry`: Generic retry wrapper for any async operation
//!
//! # Example
//!
//! ```ignore
//! use crate::llm_client::{LlmClient, RetryConfig, with_retry};
//!
//! let config = RetryConfig::default();
//! let result = with_retry(&config, || async {
//!     client.create_message(request).await
//! }, None).await;
//! ```

use crate::config::RetryPolicy;
use crate::models::{MessageRequest, MessageResponse, StreamEvent};
use anyhow::Result;
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;
use std::time::{Duration, Instant};
use uuid::Uuid;

#[cfg(test)]
pub mod mock;

// === LlmClient Trait ===

/// Type alias for boxed stream of SSE events
pub type StreamEventBox =
    Pin<Box<dyn futures_util::Stream<Item = Result<StreamEvent>> + Send + 'static>>;

/// Unified interface for LLM providers.
///
/// This trait abstracts over different LLM APIs (DeepSeek, `OpenAI`, etc.)
/// allowing the agent to work with any provider that implements this interface.
///
/// # Implementation Notes
///
/// - All methods are async and require `Send + Sync` for thread safety
/// - The `create_message_stream` method returns a pinned boxed stream for SSE
/// - Implementations should handle their own authentication and base URL configuration
#[allow(async_fn_in_trait, dead_code)] // Trait methods are part of the LLM provider interface
pub trait LlmClient: Send + Sync {
    /// Returns the provider name (e.g., "openai", "deepseek")
    fn provider_name(&self) -> &'static str;

    /// Returns the model identifier being used
    fn model(&self) -> &str;

    /// Creates a non-streaming message completion
    fn create_message(
        &self,
        request: MessageRequest,
    ) -> impl Future<Output = Result<MessageResponse>> + Send;

    /// Creates a streaming message completion
    ///
    /// Returns a stream of SSE events that should be consumed until completion.
    async fn create_message_stream(&self, request: MessageRequest) -> Result<StreamEventBox>;

    /// Optional health check to verify API connectivity
    async fn health_check(&self) -> Result<bool> {
        Ok(true)
    }
}

/// Trait for clients that support configurable retry behavior
#[allow(dead_code)] // Part of LLM provider interface, will be used by additional providers
pub trait RetryConfigurable {
    fn retry_config(&self) -> &RetryConfig;
    fn set_retry_config(&mut self, config: RetryConfig);
}

// === Authentication diagnostics ===

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AuthenticationErrorContext {
    pub provider: Option<String>,
    pub base_url_authority: Option<String>,
    pub model: Option<String>,
    pub key_source: Option<String>,
    pub key_fingerprint: Option<String>,
    pub key_kind: Option<String>,
}

impl AuthenticationErrorContext {
    #[must_use]
    pub fn new(
        provider: &str,
        base_url: &str,
        model: &str,
        key_source: &str,
        api_key: &str,
    ) -> Self {
        Self::from_parts(
            Some(provider),
            Some(base_url),
            Some(model),
            Some(key_source),
            Some(api_key),
        )
    }

    #[must_use]
    pub fn from_parts(
        provider: Option<&str>,
        base_url: Option<&str>,
        model: Option<&str>,
        key_source: Option<&str>,
        api_key: Option<&str>,
    ) -> Self {
        let api_key = api_key.and_then(non_empty_trimmed);
        Self {
            provider: provider.and_then(non_empty_trimmed).map(str::to_string),
            base_url_authority: base_url.and_then(base_url_authority),
            model: model.and_then(non_empty_trimmed).map(str::to_string),
            key_source: key_source.and_then(non_empty_trimmed).map(str::to_string),
            key_fingerprint: api_key.map(redacted_key_fingerprint),
            key_kind: api_key.map(classify_api_key_prefix).map(str::to_string),
        }
    }

    fn is_empty(&self) -> bool {
        self.provider.is_none()
            && self.base_url_authority.is_none()
            && self.model.is_none()
            && self.key_source.is_none()
            && self.key_fingerprint.is_none()
            && self.key_kind.is_none()
    }

    fn detail_segments(&self) -> Vec<String> {
        let mut segments = Vec::new();
        if let Some(provider) = self.provider.as_deref() {
            segments.push(format!("provider: {provider}"));
        }
        if let Some(authority) = self.base_url_authority.as_deref() {
            segments.push(format!("base URL authority: {authority}"));
        }
        if let Some(model) = self.model.as_deref() {
            segments.push(format!("model: {model}"));
        }
        if let Some(source) = self.key_source.as_deref() {
            segments.push(format!("key source: {source}"));
        }
        if let Some(fingerprint) = self.key_fingerprint.as_deref() {
            segments.push(format!("key fingerprint: {fingerprint}"));
        }
        if let Some(kind) = self.key_kind.as_deref() {
            segments.push(format!("key type: {kind}"));
        }
        segments
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthenticationErrorDetail {
    message: String,
    context: Option<AuthenticationErrorContext>,
}

impl AuthenticationErrorDetail {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            context: None,
        }
    }

    #[must_use]
    pub fn with_context(
        message: impl Into<String>,
        context: Option<AuthenticationErrorContext>,
    ) -> Self {
        let context = context.filter(|context| !context.is_empty());
        Self {
            message: message.into(),
            context,
        }
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    #[must_use]
    pub fn to_user_message(&self) -> String {
        let Some(context) = self.context.as_ref() else {
            return self.message.clone();
        };
        let segments = context.detail_segments();
        if segments.is_empty() {
            self.message.clone()
        } else {
            format!("{} ({})", self.message, segments.join(", "))
        }
    }
}

impl From<String> for AuthenticationErrorDetail {
    fn from(message: String) -> Self {
        Self::new(message)
    }
}

impl From<&str> for AuthenticationErrorDetail {
    fn from(message: &str) -> Self {
        Self::new(message)
    }
}

#[must_use]
pub fn classify_api_key_prefix(api_key: &str) -> &'static str {
    if api_key.starts_with("tp-") {
        "Xiaomi MiMo Token Plan key"
    } else {
        "API key"
    }
}

fn non_empty_trimmed(value: &str) -> Option<&str> {
    let value = value.trim();
    if value.is_empty() { None } else { Some(value) }
}

fn base_url_authority(base_url: &str) -> Option<String> {
    let base_url = non_empty_trimmed(base_url)?;
    let without_scheme = base_url
        .split_once("://")
        .map_or(base_url, |(_, rest)| rest);
    let authority = without_scheme.split('/').next().unwrap_or(without_scheme);
    let authority = authority
        .rsplit_once('@')
        .map_or(authority, |(_, authority)| authority);
    non_empty_trimmed(authority).map(str::to_string)
}

fn redacted_key_fingerprint(api_key: &str) -> String {
    let api_key = api_key.trim();
    let len = api_key.chars().count();
    match public_key_prefix(api_key) {
        Some(prefix) => format!("{prefix}... (len={len})"),
        None => format!("unprefixed (len={len})"),
    }
}

fn public_key_prefix(api_key: &str) -> Option<&str> {
    ["tp-", "sk-", "hf_", "hf-", "ak-", "rk-"]
        .into_iter()
        .find(|prefix| api_key.starts_with(prefix))
}

fn redact_api_key_from_message(message: &str, api_key: Option<&str>) -> String {
    let Some(api_key) = api_key.and_then(non_empty_trimmed) else {
        return message.to_string();
    };
    message.replace(api_key, "[redacted API key]")
}

// === LlmError - Classified Error Types ===

/// Classified LLM errors with retryability information.
///
/// This enum categorizes API errors to enable smart retry decisions.
/// Some errors (rate limits, transient server errors) are retryable,
/// while others (auth failures, invalid requests) should fail immediately.
#[derive(Debug)]
pub enum LlmError {
    /// Rate limit exceeded (HTTP 429)
    /// Contains optional Retry-After duration from server
    RateLimited {
        message: String,
        retry_after: Option<Duration>,
    },

    /// Server error (HTTP 5xx)
    ServerError { status: u16, message: String },

    /// Network connectivity error
    NetworkError(String),

    /// Request timed out
    Timeout(Duration),

    /// Authentication failed (HTTP 401, selected HTTP 403)
    AuthenticationError(AuthenticationErrorDetail),

    /// Authorization or provider-side blocking failed (HTTP 403)
    AuthorizationError(String),

    /// Invalid request parameters (HTTP 400)
    InvalidRequest { status: u16, message: String },

    /// Model-specific error (model not found, etc.)
    ModelError(String),

    /// Content policy violation (safety filters)
    ContentPolicyError(String),

    /// Failed to parse API response
    ParseError(String),

    /// Context length exceeded
    ContextLengthError(String),

    /// Catch-all for other errors
    Other(String),
}

impl std::fmt::Display for LlmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LlmError::RateLimited { message, .. } => write!(f, "Rate limit exceeded: {message}"),
            LlmError::ServerError { status, message } => {
                write!(f, "Server error ({status}): {message}")
            }
            LlmError::NetworkError(msg) => write!(f, "Network error: {msg}"),
            LlmError::Timeout(d) => write!(f, "Request timed out after {d:?}"),
            LlmError::AuthenticationError(auth) => {
                write!(f, "Authentication failed: {}", auth.to_user_message())
            }
            LlmError::AuthorizationError(msg) => write!(f, "Authorization failed: {msg}"),
            LlmError::InvalidRequest { status, message } => {
                write!(f, "Invalid request ({status}): {message}")
            }
            LlmError::ModelError(msg) => write!(f, "Model error: {msg}"),
            LlmError::ContentPolicyError(msg) => write!(f, "Content policy violation: {msg}"),
            LlmError::ParseError(msg) => write!(f, "Response parsing error: {msg}"),
            LlmError::ContextLengthError(msg) => write!(f, "Context length exceeded: {msg}"),
            LlmError::Other(msg) => write!(f, "LLM error: {msg}"),
        }
    }
}

impl std::error::Error for LlmError {}

impl LlmError {
    /// Determines if this error is potentially transient and worth retrying.
    ///
    /// Retryable errors:
    /// - Rate limits (with backoff)
    /// - Server errors (5xx)
    /// - Network errors (connection issues)
    /// - Timeouts
    ///
    /// Non-retryable errors:
    /// - Authentication failures
    /// - Invalid requests
    /// - Content policy violations
    /// - Context length errors
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            LlmError::RateLimited { .. }
                | LlmError::ServerError { .. }
                | LlmError::NetworkError(_)
                | LlmError::Timeout(_)
        )
    }

    /// Returns the server-suggested retry delay if available.
    ///
    /// This is typically present for rate limit errors when the server
    /// provides a Retry-After header.
    pub fn suggested_retry_delay(&self) -> Option<Duration> {
        match self {
            LlmError::RateLimited { retry_after, .. } => *retry_after,
            _ => None,
        }
    }

    /// Constructs an `LlmError` from HTTP status code and response body.
    ///
    /// Performs heuristic classification based on:
    /// - Status code (429 = rate limit, 401/403 = auth, 5xx = server error)
    /// - Response body keywords (`context_length`, `content_policy`, safety, etc.)
    pub fn from_http_response(status: u16, body: &str) -> Self {
        match status {
            429 => LlmError::RateLimited {
                message: body.to_string(),
                retry_after: None,
            },
            401 => Self::authentication_error(body),
            403 => {
                if looks_like_authentication_failure(body) {
                    Self::authentication_error(body)
                } else {
                    LlmError::AuthorizationError(body.to_string())
                }
            }
            400 => {
                // Classify 400 errors by examining the response body
                let body_lower = body.to_lowercase();
                if body_lower.contains("insufficientquota")
                    || body_lower.contains("insufficient_quota")
                    || body_lower.contains("exceeded your current quota")
                    || body_lower.contains("quota exceeded")
                {
                    LlmError::RateLimited {
                        message: body.to_string(),
                        retry_after: None,
                    }
                } else if body_lower.contains("context_length")
                    || body_lower.contains("token")
                    || body_lower.contains("too long")
                    || body_lower.contains("maximum")
                {
                    LlmError::ContextLengthError(body.to_string())
                } else if body_lower.contains("content_policy")
                    || body_lower.contains("safety")
                    || body_lower.contains("harmful")
                    || body_lower.contains("inappropriate")
                {
                    LlmError::ContentPolicyError(body.to_string())
                } else if body_lower.contains("model") && body_lower.contains("not found") {
                    LlmError::ModelError(body.to_string())
                } else {
                    LlmError::InvalidRequest {
                        status,
                        message: body.to_string(),
                    }
                }
            }
            404 => {
                if body.to_lowercase().contains("model") {
                    LlmError::ModelError(body.to_string())
                } else {
                    LlmError::InvalidRequest {
                        status,
                        message: body.to_string(),
                    }
                }
            }
            500..=599 => LlmError::ServerError {
                status,
                message: body.to_string(),
            },
            _ => LlmError::Other(format!("HTTP {status}: {body}")),
        }
    }

    #[must_use]
    pub fn authentication_error(message: impl Into<String>) -> Self {
        LlmError::AuthenticationError(AuthenticationErrorDetail::new(message))
    }

    #[must_use]
    pub fn authentication_error_with_context(
        message: impl Into<String>,
        context: Option<AuthenticationErrorContext>,
    ) -> Self {
        LlmError::AuthenticationError(AuthenticationErrorDetail::with_context(message, context))
    }

    /// Constructs an `LlmError` from HTTP response data plus request context
    /// that is safe to display when authentication fails.
    #[must_use]
    pub fn from_http_response_with_request_context(
        status: u16,
        body: &str,
        provider: Option<&str>,
        base_url: Option<&str>,
        model: Option<&str>,
        key_source: Option<&str>,
        api_key: Option<&str>,
    ) -> Self {
        let body = redact_api_key_from_message(body, api_key);
        let context =
            AuthenticationErrorContext::from_parts(provider, base_url, model, key_source, api_key);
        Self::from_http_response_with_auth_context(status, &body, Some(context))
    }

    /// Constructs an `LlmError` from HTTP status code and response body, with
    /// optional structured details for authentication failures.
    ///
    /// The `body` passed here must already be safe for user display. Prefer
    /// [`Self::from_http_response_with_request_context`] when the raw API key is
    /// available so the response body can be redacted before rendering.
    #[must_use]
    pub fn from_http_response_with_auth_context(
        status: u16,
        body: &str,
        auth_context: Option<AuthenticationErrorContext>,
    ) -> Self {
        match status {
            401 => Self::authentication_error_with_context(body, auth_context),
            403 => {
                if looks_like_authentication_failure(body) {
                    Self::authentication_error_with_context(body, auth_context)
                } else {
                    LlmError::AuthorizationError(body.to_string())
                }
            }
            _ => Self::from_http_response(status, body),
        }
    }

    /// Constructs an `LlmError` from HTTP status code, body, and optional Retry-After header.
    pub fn from_http_response_with_retry_after(
        status: u16,
        body: &str,
        retry_after: Option<Duration>,
    ) -> Self {
        let mut error = Self::from_http_response(status, body);
        if let LlmError::RateLimited {
            retry_after: ref mut ra,
            ..
        } = error
        {
            *ra = retry_after;
        }
        error
    }

    /// Constructs an `LlmError` from a reqwest error.
    pub fn from_reqwest(err: &reqwest::Error) -> Self {
        if err.is_timeout() {
            LlmError::Timeout(Duration::from_secs(0))
        } else if err.is_connect() {
            LlmError::NetworkError(format!("Connection failed: {err}"))
        } else if err.is_request() {
            LlmError::NetworkError(format!("Request failed: {err}"))
        } else {
            LlmError::Other(err.to_string())
        }
    }
}

/// Format provider HTTP error bodies before they are surfaced in the TUI.
///
/// Providers sometimes return whole HTML error pages for gateway/WAF blocks.
/// Passing those pages through raw floods the transcript and can also make a
/// provider-side 403 look like a broken API key. Keep the useful details and
/// cap everything else.
#[must_use]
pub(crate) fn sanitize_http_error_body(
    provider_label: Option<&str>,
    status: u16,
    body: &str,
) -> String {
    if let Some(message) = extract_json_error_message(body) {
        return truncate_for_error(&collapse_whitespace(&message), 2_000);
    }

    if is_probably_html(body) {
        let text = html_to_text(body);
        let lower = text.to_ascii_lowercase();
        let provider = provider_label.unwrap_or("Provider");

        // Cloudflare's "Access Denied" interstitial strips the literal word
        // "cloudflare" once tags are removed (it only survives in `<meta>`
        // attributes and the `<style>`/`<script>` blocks we discard). Arcee's
        // 403 page is exactly this shape, so also key off the WAF's stock copy
        // ("security alert", "contact support") and a Cloudflare error/ray ID.
        let error_id = extract_cloudflare_error_id(&text);
        let is_cloudflare = lower.contains("cloudflare");
        let looks_like_access_denied = lower.contains("access denied")
            && (is_cloudflare
                || lower.contains("security alert")
                || lower.contains("contact support")
                || lower.contains("contact us")
                || error_id.is_some());
        if looks_like_access_denied {
            let label = if is_cloudflare {
                "Cloudflare Access Denied"
            } else {
                "Access Denied"
            };
            let mut message = format!(
                "{provider} API returned {label} (HTTP {status}). \
                 The request was blocked before it reached the model; retry with a \
                 smaller request or fewer tools, or contact provider support"
            );
            if let Some(id) = error_id {
                message.push_str(&format!(" with ID {id}"));
            }
            message.push('.');
            return message;
        }

        let text = truncate_for_error(&collapse_whitespace(&text), 900);
        return format!("{provider} API returned an HTML error page (HTTP {status}): {text}");
    }

    truncate_for_error(&collapse_whitespace(body), 2_000)
}

fn looks_like_authentication_failure(body: &str) -> bool {
    let lower = body.to_ascii_lowercase();
    lower.contains("authentication")
        || lower.contains("unauthorized")
        || lower.contains("api key")
        || lower.contains("invalid key")
        || lower.contains("invalid token")
        || lower.contains("bearer token")
        || lower.contains("missing token")
}

fn extract_json_error_message(body: &str) -> Option<String> {
    let value: Value = serde_json::from_str(body).ok()?;
    for pointer in [
        "/error/message",
        "/error",
        "/message",
        "/detail",
        "/error_description",
    ] {
        let Some(value) = value.pointer(pointer) else {
            continue;
        };
        if let Some(message) = value.as_str() {
            if !message.trim().is_empty() {
                return Some(message.to_string());
            }
        } else if value.is_object() || value.is_array() {
            return Some(value.to_string());
        }
    }
    None
}

fn is_probably_html(body: &str) -> bool {
    let prefix = body
        .chars()
        .take(512)
        .collect::<String>()
        .to_ascii_lowercase();
    prefix.contains("<!doctype html") || prefix.contains("<html") || prefix.contains("<head")
}

fn html_to_text(html: &str) -> String {
    let without_scripts = strip_html_block(html, "script");
    let without_styles = strip_html_block(&without_scripts, "style");
    let mut text = String::with_capacity(without_styles.len().min(4096));
    let mut in_tag = false;
    for ch in without_styles.chars() {
        match ch {
            '<' => {
                in_tag = true;
                text.push(' ');
            }
            '>' => {
                in_tag = false;
                text.push(' ');
            }
            _ if !in_tag => text.push(ch),
            _ => {}
        }
    }
    decode_basic_html_entities(&collapse_whitespace(&text))
}

fn strip_html_block(input: &str, tag: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut cursor = 0usize;
    let lower = input.to_ascii_lowercase();
    let start_marker = format!("<{tag}");
    let end_marker = format!("</{tag}>");

    while let Some(relative_start) = lower[cursor..].find(&start_marker) {
        let start = cursor + relative_start;
        out.push_str(&input[cursor..start]);
        let after_start = start + start_marker.len();
        let Some(relative_end) = lower[after_start..].find(&end_marker) else {
            cursor = input.len();
            break;
        };
        cursor = after_start + relative_end + end_marker.len();
        out.push(' ');
    }
    out.push_str(&input[cursor..]);
    out
}

fn decode_basic_html_entities(input: &str) -> String {
    input
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
}

fn collapse_whitespace(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate_for_error(input: &str, max_chars: usize) -> String {
    let mut out = String::with_capacity(input.len().min(max_chars + 32));
    for (count, ch) in input.chars().enumerate() {
        if count >= max_chars {
            out.push_str("...");
            return out;
        }
        out.push(ch);
    }
    out
}

fn extract_cloudflare_error_id(text: &str) -> Option<String> {
    let mut last = None;
    for token in text.split(|ch: char| !ch.is_ascii_hexdigit()) {
        if (16..=64).contains(&token.len()) && token.bytes().any(|b| b.is_ascii_alphabetic()) {
            last = Some(token.to_string());
        }
    }
    last
}

impl From<reqwest::Error> for LlmError {
    fn from(err: reqwest::Error) -> Self {
        LlmError::from_reqwest(&err)
    }
}

impl From<serde_json::Error> for LlmError {
    fn from(err: serde_json::Error) -> Self {
        LlmError::ParseError(err.to_string())
    }
}

// === RetryConfig - Exponential Backoff Configuration ===

/// Configuration for retry behavior with exponential backoff.
///
/// This struct controls how retries are performed:
/// - Number of retry attempts
/// - Delay calculation (exponential backoff with optional jitter)
/// - Which HTTP status codes are retryable
/// - Timeout handling
///
/// # Default Values
///
/// - `enabled`: true
/// - `max_retries`: 3
/// - `initial_delay`: 1.0 seconds
/// - `max_delay`: 60.0 seconds
/// - `exponential_base`: 2.0
/// - `jitter`: true (adds randomness to prevent thundering herd)
/// - `jitter_factor`: 0.1 (10% variation)
/// - `retryable_status_codes`: [429, 500, 502, 503, 504]
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Whether retry logic is enabled
    pub enabled: bool,

    /// Maximum number of retry attempts (0 = no retries, 3 = up to 4 total attempts)
    pub max_retries: u32,

    /// Initial delay before first retry (seconds)
    pub initial_delay: f64,

    /// Maximum delay between retries (seconds)
    pub max_delay: f64,

    /// Base for exponential backoff (delay = initial * base^attempt)
    pub exponential_base: f64,

    /// Whether to add random jitter to delays
    pub jitter: bool,

    /// Jitter factor (0.1 = +/- 10% variation)
    pub jitter_factor: f64,

    /// Whether to respect server's Retry-After header
    pub respect_retry_after: bool,

    /// HTTP status codes that should trigger a retry
    #[allow(dead_code)] // Used in tests via is_retryable_status()
    pub retryable_status_codes: Vec<u16>,

    /// Timeout for individual requests (seconds, 0 = no timeout)
    #[allow(dead_code)] // Configuration field for retry consumers
    pub request_timeout: f64,

    /// Total timeout for all retry attempts (seconds, 0 = no total timeout)
    pub total_timeout: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_retries: 3,
            initial_delay: 1.0,
            max_delay: 60.0,
            exponential_base: 2.0,
            jitter: true,
            jitter_factor: 0.1,
            respect_retry_after: true,
            retryable_status_codes: vec![429, 500, 502, 503, 504],
            request_timeout: 120.0,
            total_timeout: 0.0, // No total timeout by default
        }
    }
}

#[allow(dead_code)] // Public builder API, used in tests
impl RetryConfig {
    /// Creates a new `RetryConfig` with default values
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a config with retry disabled
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            ..Default::default()
        }
    }

    /// Builder method to set max retries
    pub fn with_max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries;
        self
    }

    /// Builder method to set initial delay
    pub fn with_initial_delay(mut self, delay: f64) -> Self {
        self.initial_delay = delay;
        self
    }

    /// Builder method to set max delay
    pub fn with_max_delay(mut self, delay: f64) -> Self {
        self.max_delay = delay;
        self
    }

    /// Builder method to enable/disable jitter
    pub fn with_jitter(mut self, enabled: bool) -> Self {
        self.jitter = enabled;
        self
    }

    /// Builder method to set request timeout
    pub fn with_request_timeout(mut self, timeout: f64) -> Self {
        self.request_timeout = timeout;
        self
    }

    /// Builder method to set total timeout
    pub fn with_total_timeout(mut self, timeout: f64) -> Self {
        self.total_timeout = timeout;
        self
    }

    /// Calculates the delay for a given retry attempt.
    ///
    /// Uses exponential backoff: delay = `initial_delay` * `exponential_base^attempt`
    /// The result is capped at `max_delay` and optionally has jitter applied.
    ///
    /// # Arguments
    ///
    /// * `attempt` - Zero-based attempt number (0 = first retry)
    ///
    /// # Returns
    ///
    /// Duration to wait before the next retry attempt
    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        let exponent = i32::try_from(attempt).unwrap_or(i32::MAX);
        let base_delay = self.initial_delay * self.exponential_base.powi(exponent);
        let capped_delay = base_delay.min(self.max_delay);

        let final_delay = if self.jitter {
            // Add random jitter to prevent thundering herd problem
            let jitter_range = capped_delay * self.jitter_factor;
            // Use UUID v4 entropy for jitter randomness.
            let bytes = *Uuid::new_v4().as_bytes();
            let sample = u16::from_le_bytes([bytes[0], bytes[1]]);
            let random_factor = f64::from(sample) / f64::from(u16::MAX); // 0.0 to 1.0
            let jitter = jitter_range * (2.0 * random_factor - 1.0); // -range to +range

            (capped_delay + jitter).max(0.0)
        } else {
            capped_delay
        };

        Duration::from_secs_f64(final_delay)
    }

    /// Checks if a given HTTP status code should trigger a retry
    pub fn is_retryable_status(&self, status: u16) -> bool {
        self.retryable_status_codes.contains(&status)
    }
}

/// Converts from the existing `RetryPolicy` in config
impl From<RetryPolicy> for RetryConfig {
    fn from(policy: RetryPolicy) -> Self {
        Self {
            enabled: policy.enabled,
            max_retries: policy.max_retries,
            initial_delay: policy.initial_delay,
            max_delay: policy.max_delay,
            exponential_base: policy.exponential_base,
            ..Default::default()
        }
    }
}

/// Converts back to `RetryPolicy` for compatibility
impl From<RetryConfig> for RetryPolicy {
    fn from(config: RetryConfig) -> Self {
        Self {
            enabled: config.enabled,
            max_retries: config.max_retries,
            initial_delay: config.initial_delay,
            max_delay: config.max_delay,
            exponential_base: config.exponential_base,
        }
    }
}

// === Retry Error and Result Types ===

/// Error returned when all retry attempts have been exhausted.
#[derive(Debug)]
pub struct RetryError {
    /// The last error encountered
    pub last_error: LlmError,

    /// Total number of attempts made
    pub attempts: u32,

    /// Total time spent across all attempts
    pub total_time: Duration,
}

impl std::fmt::Display for RetryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Retry exhausted after {} attempts ({:?}): {}",
            self.attempts, self.total_time, self.last_error
        )
    }
}

impl std::error::Error for RetryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.last_error)
    }
}

/// Result type for retry operations
pub type RetryResult<T> = Result<T, RetryError>;

/// Callback type for retry notifications
///
/// Called before each retry with:
/// - The error that triggered the retry
/// - The attempt number (0-based)
/// - The delay before the next attempt
pub type RetryCallback = Box<dyn Fn(&LlmError, u32, Duration) + Send + Sync>;

// === with_retry - Generic Retry Wrapper ===

/// Executes an async operation with configurable retry logic.
///
/// This function wraps any async operation that returns `Result<T, LlmError>`
/// and automatically retries on transient failures using exponential backoff.
///
/// # Arguments
///
/// * `config` - Retry configuration (delays, max attempts, etc.)
/// * `operation` - Async closure to execute (will be called multiple times on retry)
/// * `callback` - Optional callback for retry notifications (logging, metrics, etc.)
///
/// # Returns
///
/// * `Ok(T)` - The successful result from the operation
/// * `Err(RetryError)` - All retries exhausted or non-retryable error encountered
///
/// # Example
///
/// ```ignore
/// let result = with_retry(
///     &config,
///     || async { client.send_request(&req).await },
///     Some(Box::new(|err, attempt, delay| {
///         eprintln!("Retry {} after {:?}: {}", attempt, delay, err);
///     })),
/// ).await;
/// ```
pub async fn with_retry<F, Fut, T>(
    config: &RetryConfig,
    mut operation: F,
    callback: Option<RetryCallback>,
) -> RetryResult<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, LlmError>>,
{
    // If retries are disabled, just run once
    if !config.enabled {
        return operation().await.map_err(|e| RetryError {
            last_error: e,
            attempts: 1,
            total_time: Duration::ZERO,
        });
    }

    let start_time = Instant::now();
    let total_timeout = if config.total_timeout > 0.0 {
        Some(Duration::from_secs_f64(config.total_timeout))
    } else {
        None
    };

    let mut last_error: Option<LlmError> = None;

    // Attempt 0 is the first try, then up to max_retries additional attempts
    for attempt in 0..=config.max_retries {
        // Check total timeout
        if let Some(timeout) = total_timeout
            && start_time.elapsed() >= timeout
        {
            return Err(RetryError {
                last_error: last_error.unwrap_or(LlmError::Timeout(timeout)),
                attempts: attempt,
                total_time: start_time.elapsed(),
            });
        }

        match operation().await {
            Ok(result) => return Ok(result),
            Err(err) => {
                // Non-retryable errors fail immediately
                if !err.is_retryable() {
                    return Err(RetryError {
                        last_error: err,
                        attempts: attempt + 1,
                        total_time: start_time.elapsed(),
                    });
                }

                // Last attempt - no more retries
                if attempt >= config.max_retries {
                    return Err(RetryError {
                        last_error: err,
                        attempts: attempt + 1,
                        total_time: start_time.elapsed(),
                    });
                }

                // Calculate delay
                // Use server's Retry-After if available and configured
                let base_delay = config.delay_for_attempt(attempt);
                let delay = if config.respect_retry_after {
                    err.suggested_retry_delay().unwrap_or(base_delay)
                } else {
                    base_delay
                };

                // Notify callback if provided
                if let Some(ref cb) = callback {
                    cb(&err, attempt, delay);
                }

                last_error = Some(err);

                // Wait before retrying
                tokio::time::sleep(delay).await;
            }
        }
    }

    // Should not reach here, but handle gracefully
    Err(RetryError {
        last_error: last_error.unwrap_or(LlmError::Other("Unknown retry error".to_string())),
        attempts: config.max_retries + 1,
        total_time: start_time.elapsed(),
    })
}

/// Simplified version of `with_retry` without callback
#[allow(dead_code)] // Convenience wrapper for with_retry
pub async fn with_retry_simple<F, Fut, T>(config: &RetryConfig, operation: F) -> RetryResult<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, LlmError>>,
{
    with_retry(config, operation, None).await
}

// === Utility Functions ===

/// Parses the Retry-After header value into a Duration.
///
/// Supports both:
/// - Seconds as integer: "120" -> 120 seconds
/// - HTTP-date format: "Wed, 21 Oct 2015 07:28:00 GMT" (not implemented, returns None)
pub fn parse_retry_after(value: &str) -> Option<Duration> {
    // Try parsing as seconds
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }

    // Try parsing as float seconds
    if let Ok(seconds) = value.parse::<f64>() {
        return Some(Duration::from_secs_f64(seconds));
    }

    // HTTP-date format not supported yet
    // Could use chrono or httpdate crate if needed
    None
}

/// Extracts Retry-After duration from response headers
pub fn extract_retry_after(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    headers
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(parse_retry_after)
}

// === Tests ===

#[cfg(test)]
mod tests {}
