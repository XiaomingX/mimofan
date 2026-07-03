//! HTTP client for DeepSeek's OpenAI-compatible Chat Completions API.
//!
//! DeepSeek documents `/chat/completions` as the primary endpoint, and this
//! client now routes all normal traffic through that surface.

use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use base64::{Engine as _, engine::general_purpose};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::Mutex as AsyncMutex;

use mimofan_config::catalog::{
    CatalogOffering, CatalogRefreshError, CatalogSource, CatalogStatus, ProviderCatalogCache,
    ProviderCatalogDelta, base_url_fingerprint, now_unix,
};
use mimofan_config::route::ReadyRouteCandidate;

use crate::config::{ApiProvider, Config, RetryPolicy, wire_model_for_provider};
use crate::llm_client::{
    LlmClient, LlmError, RetryConfig as LlmRetryConfig, extract_retry_after,
    sanitize_http_error_body, with_retry,
};
use crate::logging;
use crate::models::{
    ContentBlock, Message, MessageRequest, MessageResponse, ServerToolUsage, SystemPrompt, Usage,
};

pub(super) fn to_api_tool_name(name: &str) -> String {
    let mut out = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch);
        } else if ch == '-' {
            out.push_str("--");
        } else {
            out.push_str("-x");
            out.push_str(&format!("{:06X}", ch as u32));
            out.push('-');
        }
    }
    out
}

pub(super) fn from_api_tool_name(name: &str) -> String {
    let mut out = String::new();
    let mut iter = name.chars().peekable();
    while let Some(ch) = iter.next() {
        if ch != '-' {
            out.push(ch);
            continue;
        }
        if let Some('-') = iter.peek().copied() {
            iter.next();
            out.push('-');
            continue;
        }
        if iter.peek().copied() == Some('x') {
            iter.next();
            let mut hex = String::new();
            for _ in 0..6 {
                if let Some(h) = iter.next() {
                    hex.push(h);
                } else {
                    break;
                }
            }
            // Only decode if we got exactly 6 hex digits (matching encoder output).
            // Fewer digits means a truncated/malformed sequence — pass through as-is.
            if hex.len() == 6
                && let Ok(code) = u32::from_str_radix(&hex, 16)
                && let Some(decoded) = std::char::from_u32(code)
            {
                if let Some('-') = iter.peek().copied() {
                    iter.next();
                }
                out.push(decoded);
                continue;
            }
            out.push('-');
            out.push('x');
            out.push_str(&hex);
            continue;
        }
        out.push('-');
    }

    // Second pass: decode bare hex escapes (e.g. `x00002E`) that the model
    // may produce when it mangles the `-x00002E-` delimiter form.  Only
    // decode when the resulting character is one that `to_api_tool_name`
    // would have encoded (not alphanumeric, not `_`, not `-`).
    decode_bare_hex_escapes(&out)
}

/// Decode bare `x[0-9A-Fa-f]{6}` sequences (optionally followed by `-`)
/// that survive the standard delimiter-based pass.  This handles cases
/// where the model strips or replaces the leading `-` of `-x00002E-`.
pub(super) fn decode_bare_hex_escapes(input: &str) -> String {
    use regex::Regex;
    use std::sync::OnceLock;

    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"x([0-9A-Fa-f]{6})-?").expect("valid regex pattern"));

    let result = re.replace_all(input, |caps: &regex::Captures| {
        let hex = &caps[1];
        if let Ok(code) = u32::from_str_radix(hex, 16)
            && let Some(decoded) = std::char::from_u32(code)
        {
            // Only decode characters that to_api_tool_name would have encoded
            if !decoded.is_ascii_alphanumeric() && decoded != '_' && decoded != '-' {
                return decoded.to_string();
            }
        }
        // Not a character we'd encode — leave as-is
        caps[0].to_string()
    });
    result.into_owned()
}

// === Types ===

/// Model descriptor returned by the provider's `/v1/models` endpoint.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AvailableModel {
    pub id: String,
    pub owned_by: Option<String>,
    pub created: Option<u64>,
}

/// Request payload for Xiaomi MiMo speech synthesis models.
///
/// MiMo-V2.5-TTS / MiMo-V2-TTS use the OpenAI-compatible
/// `/v1/chat/completions` endpoint: the optional style/voice instruction is
/// sent as a `user` message, while the text to synthesize is sent as an
/// `assistant` message.
#[derive(Debug, Clone)]
pub struct SpeechSynthesisRequest {
    pub model: String,
    pub text: String,
    pub instruction: Option<String>,
    pub audio_format: String,
    pub voice: Option<String>,
}

/// Decoded speech synthesis result.
#[derive(Debug, Clone)]
pub struct SpeechSynthesisResponse {
    pub model: String,
    pub audio_format: String,
    pub audio_bytes: Vec<u8>,
    pub transcript: Option<String>,
    pub voice: Option<String>,
}

/// Client for DeepSeek's OpenAI-compatible APIs.
#[must_use]
pub struct DeepSeekClient {
    pub(super) http_client: reqwest::Client,
    api_key: String,
    pub(super) base_url: String,
    pub(super) api_provider: ApiProvider,
    retry: RetryPolicy,
    default_model: String,
    connection_health: Arc<AsyncMutex<ConnectionHealth>>,
    rate_limiter: Arc<AsyncMutex<TokenBucket>>,
    path_suffix: Option<String>,
    pub(super) reasoning_stream_style: Option<String>,
    pub(super) stream_idle_timeout: Duration,
}

const CONNECTION_FAILURE_THRESHOLD: u32 = 2;
const RECOVERY_PROBE_COOLDOWN: Duration = Duration::from_secs(15);

const DEFAULT_CLIENT_RATE_LIMIT_RPS: f64 = 8.0;
const DEFAULT_CLIENT_RATE_LIMIT_BURST: f64 = 16.0;
const ALLOW_INSECURE_HTTP_ENV: &str = "DEEPSEEK_ALLOW_INSECURE_HTTP";

pub(super) const SSE_BACKPRESSURE_HIGH_WATERMARK: usize = 8 * 1024 * 1024; // 8 MB
pub(super) const SSE_BACKPRESSURE_SLEEP_MS: u64 = 10;
pub(super) const SSE_MAX_LINES_PER_CHUNK: usize = 256;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConnectionState {
    Healthy,
    Degraded,
    Recovering,
}

#[derive(Debug)]
struct ConnectionHealth {
    state: ConnectionState,
    consecutive_failures: u32,
    last_failure: Option<Instant>,
    last_success: Option<Instant>,
    last_probe: Option<Instant>,
}

impl Default for ConnectionHealth {
    fn default() -> Self {
        Self {
            state: ConnectionState::Healthy,
            consecutive_failures: 0,
            last_failure: None,
            last_success: None,
            last_probe: None,
        }
    }
}

#[derive(Debug)]
struct TokenBucket {
    enabled: bool,
    capacity: f64,
    tokens: f64,
    refill_per_sec: f64,
    last_refill: Instant,
}

impl TokenBucket {
    fn from_env() -> Self {
        let rps = std::env::var("DEEPSEEK_RATE_LIMIT_RPS")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(DEFAULT_CLIENT_RATE_LIMIT_RPS)
            .max(0.0);
        let burst = std::env::var("DEEPSEEK_RATE_LIMIT_BURST")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(DEFAULT_CLIENT_RATE_LIMIT_BURST)
            .max(1.0);
        let enabled = rps > 0.0;
        Self {
            enabled,
            capacity: burst,
            tokens: burst,
            refill_per_sec: rps,
            last_refill: Instant::now(),
        }
    }

    fn refill(&mut self, now: Instant) {
        if !self.enabled {
            return;
        }
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.last_refill = now;
        self.tokens = (self.tokens + elapsed * self.refill_per_sec).min(self.capacity);
    }

    fn delay_until_available(&mut self, tokens: f64) -> Option<Duration> {
        if !self.enabled {
            return None;
        }
        let now = Instant::now();
        self.refill(now);
        if self.tokens >= tokens {
            self.tokens -= tokens;
            return None;
        }
        let needed = tokens - self.tokens;
        self.tokens = 0.0;
        if self.refill_per_sec <= 0.0 {
            return Some(Duration::from_secs(1));
        }
        Some(Duration::from_secs_f64(needed / self.refill_per_sec))
    }
}

fn apply_request_success(health: &mut ConnectionHealth, now: Instant) -> bool {
    let recovered = health.state != ConnectionState::Healthy;
    health.state = ConnectionState::Healthy;
    health.consecutive_failures = 0;
    health.last_success = Some(now);
    recovered
}

fn apply_request_failure(health: &mut ConnectionHealth, now: Instant) {
    health.consecutive_failures = health.consecutive_failures.saturating_add(1);
    health.last_failure = Some(now);
    if health.consecutive_failures >= CONNECTION_FAILURE_THRESHOLD {
        health.state = ConnectionState::Degraded;
    }
}

fn mark_recovery_probe_if_due(health: &mut ConnectionHealth, now: Instant) -> bool {
    if health.state == ConnectionState::Healthy {
        return false;
    }
    if health
        .last_probe
        .is_some_and(|last| now.duration_since(last) < RECOVERY_PROBE_COOLDOWN)
    {
        return false;
    }
    health.last_probe = Some(now);
    health.state = ConnectionState::Recovering;
    true
}

fn buffer_pool() -> &'static StdMutex<Vec<Vec<u8>>> {
    static POOL: OnceLock<StdMutex<Vec<Vec<u8>>>> = OnceLock::new();
    POOL.get_or_init(|| StdMutex::new(Vec::new()))
}

fn acquire_stream_buffer() -> Vec<u8> {
    if let Ok(mut pool) = buffer_pool().lock() {
        pool.pop().unwrap_or_else(|| Vec::with_capacity(8192))
    } else {
        Vec::with_capacity(8192)
    }
}

fn release_stream_buffer(mut buf: Vec<u8>) {
    buf.clear();
    if buf.capacity() > 256 * 1024 {
        buf.shrink_to(256 * 1024);
    }
    if let Ok(mut pool) = buffer_pool().lock()
        && pool.len() < 8
    {
        pool.push(buf);
    }
}

impl Clone for DeepSeekClient {
    fn clone(&self) -> Self {
        Self {
            http_client: self.http_client.clone(),
            api_key: self.api_key.clone(),
            base_url: self.base_url.clone(),
            api_provider: self.api_provider,
            retry: self.retry.clone(),
            default_model: self.default_model.clone(),
            connection_health: self.connection_health.clone(),
            rate_limiter: self.rate_limiter.clone(),
            path_suffix: self.path_suffix.clone(),
            reasoning_stream_style: self.reasoning_stream_style.clone(),
            stream_idle_timeout: self.stream_idle_timeout,
        }
    }
}

// === Helpers ===

/// Maximum bytes to read from an error response body (64 KB).
pub(super) const ERROR_BODY_MAX_BYTES: usize = 64 * 1024;

/// Read an error response body with a size limit to prevent unbounded allocation.
pub(super) async fn bounded_error_text(response: reqwest::Response, max_bytes: usize) -> String {
    use futures_util::StreamExt;
    let mut stream = response.bytes_stream();
    let mut buf = Vec::with_capacity(max_bytes.min(8192));
    while let Some(chunk) = stream.next().await {
        let Ok(chunk) = chunk else { break };
        let remaining = max_bytes.saturating_sub(buf.len());
        if remaining == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
    }
    String::from_utf8_lossy(&buf).into_owned()
}

fn validate_base_url_security(base_url: &str) -> Result<()> {
    let display_base_url = redact_url_for_display(base_url);
    if base_url.starts_with("https://")
        || base_url.starts_with("http://localhost")
        || base_url.starts_with("http://127.0.0.1")
        || base_url.starts_with("http://[::1]")
    {
        return Ok(());
    }

    if base_url.starts_with("http://")
        && std::env::var(ALLOW_INSECURE_HTTP_ENV)
            .ok()
            .as_deref()
            .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
    {
        logging::warn(format!(
            "Using insecure HTTP base URL because {ALLOW_INSECURE_HTTP_ENV} is set"
        ));
        return Ok(());
    }

    if base_url.starts_with("http://") {
        anyhow::bail!(
            "Refusing insecure base URL '{display_base_url}'.\n\
             \n\
             Loopback hosts (localhost, 127.0.0.1, [::1]) are auto-allowed.\n\
             For other trusted local hosts (LAN, llama.cpp on a private IP, etc.)\n\
             set the env var `{ALLOW_INSECURE_HTTP_ENV}=1` in the shell that runs deepseek and re-run.\n\
             \n\
             Example: `{ALLOW_INSECURE_HTTP_ENV}=1 deepseek` (note the underscores).",
        );
    }

    anyhow::bail!(
        "Refusing base URL '{display_base_url}': only HTTPS (or explicitly allowed HTTP) URLs are supported.",
    )
}

pub(crate) fn redact_url_for_display(url: &str) -> String {
    let Ok(mut parsed) = reqwest::Url::parse(url) else {
        return url.to_string();
    };
    if !parsed.username().is_empty() || parsed.password().is_some() {
        let _ = parsed.set_username("***");
        let _ = parsed.set_password(Some("***"));
    }
    if parsed.query().is_none() {
        return parsed.to_string();
    }
    let pairs: Vec<(String, String)> = parsed
        .query_pairs()
        .map(|(key, value)| {
            let value = if is_sensitive_url_query_key(&key) {
                "***".to_string()
            } else {
                value.into_owned()
            };
            (key.into_owned(), value)
        })
        .collect();
    parsed.set_query(None);
    let mut query = parsed.query_pairs_mut();
    for (key, value) in pairs {
        query.append_pair(&key, &value);
    }
    drop(query);
    parsed.to_string()
}

fn is_sensitive_url_query_key(key: &str) -> bool {
    let normalized = key.trim().replace(['-', '.'], "_").to_ascii_lowercase();
    matches!(
        normalized.as_str(),
        "api_key"
            | "apikey"
            | "access_token"
            | "auth_token"
            | "authorization"
            | "bearer"
            | "client_secret"
            | "credential"
            | "id_token"
            | "password"
            | "refresh_token"
            | "secret"
            | "token"
    ) || normalized.ends_with("_api_key")
        || normalized.ends_with("_authorization")
        || normalized.ends_with("_password")
        || normalized.ends_with("_secret")
        || normalized.ends_with("_token")
}

pub(super) fn versioned_base_url(base_url: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    if base_url_has_version_suffix(trimmed) {
        trimmed.to_string()
    } else {
        format!("{trimmed}/v1")
    }
}

fn unversioned_base_url(base_url: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    trimmed
        .rsplit_once('/')
        .filter(|(_, segment)| is_version_segment(segment))
        .map(|(base, _)| base)
        .unwrap_or(trimmed)
        .to_string()
}

fn base_url_has_version_suffix(trimmed: &str) -> bool {
    trimmed.rsplit('/').next().is_some_and(is_version_segment)
}

fn is_version_segment(segment: &str) -> bool {
    segment.eq_ignore_ascii_case("beta")
        || segment
            .strip_prefix('v')
            .or_else(|| segment.strip_prefix('V'))
            .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|ch| ch.is_ascii_digit()))
}

pub(super) fn api_url(base_url: &str, path: &str) -> String {
    api_url_with_suffix(base_url, path, None)
}

pub(super) fn api_url_with_suffix(base_url: &str, path: &str, path_suffix: Option<&str>) -> String {
    let path = path.trim_start_matches('/');
    if path.starts_with("beta/") {
        return format!("{}/{}", unversioned_base_url(base_url), path);
    }
    if let ("chat/completions", Some(suffix)) = (path, path_suffix) {
        return format!(
            "{}/{}",
            unversioned_base_url(base_url),
            suffix.trim_start_matches('/')
        );
    }
    let mut versioned = versioned_base_url(base_url);
    // The /beta suffix is not a real API version — it is an
    // opt-in surface for beta features.  Only paths with an
    // explicit `beta/` prefix should hit the beta surface;
    // everything else (models, chat/completions, health, …)
    // must go to the standard /v1 surface.
    if versioned.ends_with("beta") {
        versioned = format!("{}/v1", unversioned_base_url(base_url));
    }
    format!("{}/{}", versioned.trim_end_matches('/'), path)
}

fn normalize_audio_format(format: &str) -> String {
    let normalized = format.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        "wav".to_string()
    } else {
        normalized
    }
}

fn parse_speech_audio_response(payload: &Value) -> Result<(Vec<u8>, Option<String>)> {
    let audio = payload
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| {
            choice
                .get("message")
                .and_then(|message| message.get("audio"))
                .or_else(|| choice.get("delta").and_then(|delta| delta.get("audio")))
        })
        .or_else(|| payload.get("audio"))
        .context("Speech synthesis response did not include choices[0].message.audio")?;

    let data = audio
        .get("data")
        .and_then(Value::as_str)
        .context("Speech synthesis response did not include audio.data")?
        .trim();
    let data = data
        .split_once(',')
        .map(|(_, base64)| base64.trim())
        .unwrap_or(data);
    let audio_bytes = general_purpose::STANDARD
        .decode(data)
        .context("Failed to decode speech audio base64 data")?;
    let transcript = audio
        .get("transcript")
        .and_then(Value::as_str)
        .map(str::to_string);

    Ok((audio_bytes, transcript))
}

fn build_speech_synthesis_body(
    model: &str,
    text: &str,
    instruction: Option<&str>,
    audio: Value,
) -> Value {
    let mut messages = Vec::new();
    if let Some(instruction) = instruction.map(str::trim).filter(|value| !value.is_empty()) {
        messages.push(json!({
            "role": "user",
            "content": instruction,
        }));
    }
    messages.push(json!({
        "role": "assistant",
        "content": text,
    }));

    json!({
        "model": model,
        "messages": messages,
        "audio": audio,
    })
}

// === DeepSeekClient ===

/// Returns true when DEEPSEEK_FORCE_HTTP1 is set to a truthy value
/// (`1`, `true`, `yes`, `on`, case-insensitive). Used by `build_http_client`
/// to opt out of HTTP/2 entirely when DeepSeek's edge mishandles long-lived H2
/// streams (#103). Anything else (unset, `0`, `false`, ...) leaves HTTP/2 on.
fn force_http1_from_env() -> bool {
    std::env::var("DEEPSEEK_FORCE_HTTP1")
        .ok()
        .map(|v| v.trim().to_ascii_lowercase())
        .is_some_and(|v| matches!(v.as_str(), "1" | "true" | "yes" | "on"))
}

/// Read `SSL_CERT_FILE` and add its contents as extra root
/// certificates on the reqwest builder (#418). Tries the PEM-bundle
/// parser first (covers single-cert files too), then falls back to
/// DER. All failures log a warning and return the builder unchanged
/// so a malformed env var degrades gracefully.
fn add_extra_root_certs(
    mut builder: reqwest::ClientBuilder,
    cert_path: &str,
) -> reqwest::ClientBuilder {
    let bytes = match std::fs::read(cert_path) {
        Ok(b) => b,
        Err(err) => {
            logging::warn(format!(
                "SSL_CERT_FILE={cert_path} could not be read: {err}"
            ));
            return builder;
        }
    };

    if let Ok(certs) = reqwest::Certificate::from_pem_bundle(&bytes) {
        let added = certs.len();
        for cert in certs {
            builder = builder.add_root_certificate(cert);
        }
        logging::info(format!(
            "SSL_CERT_FILE={cert_path} loaded ({added} cert(s))"
        ));
        return builder;
    }

    match reqwest::Certificate::from_der(&bytes) {
        Ok(cert) => {
            builder = builder.add_root_certificate(cert);
            logging::info(format!("SSL_CERT_FILE={cert_path} loaded (1 DER cert)"));
        }
        Err(err) => {
            logging::warn(format!(
                "SSL_CERT_FILE={cert_path} could not be parsed as PEM bundle or DER: {err}"
            ));
        }
    }
    builder
}

impl DeepSeekClient {
    /// Create a DeepSeek client from CLI configuration.
    pub fn new(config: &Config) -> Result<Self> {
        Self::from_parts(config.deepseek_base_url(), config.default_model(), config)
    }

    /// Create a DeepSeek client whose transport is bound to a runtime-resolved
    /// route (#3384).
    ///
    /// The base URL and default model come from the executable `candidate`, so
    /// the client talks to exactly the endpoint and wire model the resolver
    /// chose instead of re-deriving them from `Config`. Secrets stay in
    /// `Config`: `ReadyRouteCandidate` is secret-free by design (it carries only
    /// an auth-source *class*), so the API key and provider are still read from
    /// `config`.
    pub fn from_candidate(config: &Config, candidate: &ReadyRouteCandidate) -> Result<Self> {
        Self::from_parts(
            candidate.endpoint.base_url.clone(),
            candidate.wire_model_id.as_str().to_string(),
            config,
        )
    }

    /// Shared constructor body for [`Self::new`] and [`Self::from_candidate`].
    ///
    /// `base_url` and `default_model` are the only inputs that differ between
    /// the two entry points; everything else (auth, provider, retry, headers,
    /// timeouts) is derived from `config` so the two paths cannot drift.
    fn from_parts(base_url: String, default_model: String, config: &Config) -> Result<Self> {
        let api_key = config.deepseek_api_key()?;
        let api_provider = config.api_provider();
        validate_base_url_security(&base_url)?;
        let retry = config.retry_policy();
        let stream_idle_timeout = Duration::from_secs(config.stream_chunk_timeout_secs());
        let http_headers = config.http_headers();
        let insecure_skip_tls_verify = config.insecure_skip_tls_verify();
        let path_suffix = config
            .provider_config_for(api_provider)
            .and_then(|p| p.path_suffix.clone());
        let reasoning_stream_style = config
            .provider_config_for(api_provider)
            .and_then(|p| p.reasoning_stream_style.clone());

        logging::info(format!("API provider: {}", api_provider.as_str()));
        logging::info(format!(
            "API base URL: {}",
            redact_url_for_display(&base_url)
        ));
        if let Some(suffix) = &path_suffix {
            logging::info(format!("API path suffix override: {suffix}"));
        }
        if !http_headers.is_empty() {
            logging::info(format!(
                "{} custom HTTP header(s) configured",
                http_headers.len()
            ));
        }
        if insecure_skip_tls_verify {
            logging::warn(format!(
                "TLS certificate verification cannot be disabled for provider {}; use SSL_CERT_FILE with a trusted custom CA bundle instead",
                api_provider.as_str()
            ));
            bail!(
                "TLS certificate verification cannot be disabled for provider {}; configure SSL_CERT_FILE with a trusted custom CA bundle instead",
                api_provider.as_str()
            );
        }
        logging::info(format!(
            "Retry policy: enabled={}, max_retries={}, initial_delay={}s, max_delay={}s",
            retry.enabled, retry.max_retries, retry.initial_delay, retry.max_delay
        ));

        let http_client =
            Self::build_http_client(&api_key, &http_headers, api_provider, &base_url)?;

        Ok(Self {
            http_client,
            api_key,
            base_url,
            api_provider,
            retry,
            default_model,
            connection_health: Arc::new(AsyncMutex::new(ConnectionHealth::default())),
            rate_limiter: Arc::new(AsyncMutex::new(TokenBucket::from_env())),
            path_suffix,
            reasoning_stream_style,
            stream_idle_timeout,
        })
    }

    fn build_http_client(
        api_key: &str,
        extra_headers: &HashMap<String, String>,
        api_provider: ApiProvider,
        base_url: &str,
    ) -> Result<reqwest::Client> {
        let headers = build_default_headers(api_key, extra_headers, api_provider, base_url)?;
        // The ChatGPT Codex backend sits behind Cloudflare bot protection that
        // only admits the Codex CLI's user agent; present a codex_cli_rs UA on
        // that path so the request is handled like the official client.
        let user_agent: &str = if api_provider == ApiProvider::XiaomiMimo {
            concat!(
                "codex_cli_rs/0.137.0 (mimofan ",
                env!("CARGO_PKG_VERSION"),
                ")"
            )
        } else {
            concat!(
                "Mozilla/5.0 (compatible; mimofan/",
                env!("CARGO_PKG_VERSION"),
                "; +https://github.com/XiaomingX/mimofan)"
            )
        };
        let mut builder = crate::tls::reqwest_client_builder()
            .default_headers(headers)
            .user_agent(user_agent)
            .connect_timeout(Duration::from_secs(30))
            .tcp_keepalive(Some(Duration::from_secs(30)))
            .http2_keep_alive_interval(Some(Duration::from_secs(15)))
            .http2_keep_alive_timeout(Duration::from_secs(20))
            .min_tls_version(reqwest::tls::Version::TLS_1_2);
        if force_http1_from_env() {
            logging::info("DEEPSEEK_FORCE_HTTP1=1 — pinning HTTP client to HTTP/1.1");
            builder = builder.http1_only();
        }
        if let Ok(cert_path) = std::env::var("SSL_CERT_FILE")
            && !cert_path.is_empty()
        {
            builder = add_extra_root_certs(builder, &cert_path);
        }
        builder.build().map_err(Into::into)
    }
}

fn build_default_headers(
    api_key: &str,
    extra_headers: &HashMap<String, String>,
    api_provider: ApiProvider,
    base_url: &str,
) -> Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    let api_key = api_key.trim();
    if api_provider_uses_anthropic_messages(api_provider) {
        // #3014: the Messages API authenticates with `x-api-key` (never
        // `Authorization: Bearer`) and pins the wire contract via
        // `anthropic-version`.
        headers.insert(
            HeaderName::from_static("anthropic-version"),
            HeaderValue::from_static("2023-06-01"),
        );
    }
    let auth_header_name =
        if !api_key.is_empty() && api_provider_uses_anthropic_messages(api_provider) {
            Some(HeaderName::from_static("x-api-key"))
        } else if !api_key.is_empty()
            && api_provider == ApiProvider::XiaomiMimo
            && (xiaomi_mimo_base_url_uses_token_plan(base_url)
                || xiaomi_mimo_api_key_uses_token_plan(api_key))
        {
            Some(HeaderName::from_static("api-key"))
        } else if !api_key.is_empty() {
            Some(AUTHORIZATION)
        } else {
            None
        };
    if let Some(header_name) = auth_header_name.as_ref() {
        let header_value = if *header_name == AUTHORIZATION {
            HeaderValue::from_str(&format!("Bearer {api_key}"))?
        } else {
            HeaderValue::from_str(api_key)?
        };
        headers.insert(header_name.clone(), header_value);
    }
    for (name, value) in extra_headers {
        let name = name.trim();
        let value = value.trim();
        if name.is_empty() || value.is_empty() {
            continue;
        }
        let header_name = HeaderName::from_bytes(name.as_bytes())?;
        if header_name == AUTHORIZATION
            || header_name == CONTENT_TYPE
            || auth_header_name.as_ref() == Some(&header_name)
            || (auth_header_name.is_some() && is_auth_dialect_header(&header_name))
        {
            continue;
        }
        headers.insert(header_name, HeaderValue::from_str(value)?);
    }
    Ok(headers)
}

fn is_auth_dialect_header(header_name: &HeaderName) -> bool {
    header_name == AUTHORIZATION
        || header_name == HeaderName::from_static("api-key")
        || header_name == HeaderName::from_static("x-api-key")
}

fn api_provider_uses_anthropic_messages(api_provider: ApiProvider) -> bool {
    matches!(api_provider, ApiProvider::XiaomiMimo)
}

fn api_provider_skips_models_probe(api_provider: ApiProvider) -> bool {
    matches!(api_provider, ApiProvider::XiaomiMimo)
}

fn translation_system_prompt(target_language: &str) -> String {
    format!(
        include_str!("prompts/translator.md"),
        target_language = target_language
    )
}

fn translation_message_request(text: &str, model: String, target_language: &str) -> MessageRequest {
    MessageRequest {
        model,
        messages: vec![Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: text.to_string(),
                cache_control: None,
            }],
        }],
        max_tokens: 4096,
        system: Some(SystemPrompt::Text(translation_system_prompt(
            target_language,
        ))),
        tools: None,
        tool_choice: None,
        metadata: None,
        thinking: None,
        reasoning_effort: Some("off".to_string()),
        stream: Some(false),
        temperature: Some(0.1),
        top_p: None,
    }
}

fn translation_text_from_response(response: &MessageResponse) -> Result<String> {
    let translated = response
        .content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
        .trim()
        .to_string();
    if translated.is_empty() {
        bail!("translate: Anthropic Messages response did not contain text content");
    }
    Ok(translated)
}

fn xiaomi_mimo_base_url_uses_token_plan(base_url: &str) -> bool {
    let normalized = base_url.trim().to_ascii_lowercase();
    let without_scheme = normalized
        .strip_prefix("https://")
        .or_else(|| normalized.strip_prefix("http://"))
        .unwrap_or(&normalized);
    let host = without_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default();
    let host = host.split(':').next().unwrap_or(host);
    host.starts_with("token-plan-") && host.ends_with(".xiaomimimo.com")
}

fn xiaomi_mimo_api_key_uses_token_plan(api_key: &str) -> bool {
    api_key.trim_start().starts_with("tp-")
}

impl DeepSeekClient {
    /// Returns the API base URL used by this client.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Returns the active API provider for this client.
    pub fn api_provider(&self) -> ApiProvider {
        self.api_provider
    }

    /// Translate text to the requested target language using a focused
    /// non-streaming chat completion call on the supplied model.
    ///
    /// This is a lightweight translation service — no tool calls, no
    /// streaming, no conversation history. The dedicated translation agent
    /// receives the source text and returns only the translated result.
    pub async fn translate(
        &self,
        text: &str,
        model: &str,
        target_language: &str,
    ) -> Result<String> {
        let model = wire_model_for_provider(self.api_provider, model);
        if api_provider_uses_anthropic_messages(self.api_provider) {
            let response = self
                .handle_anthropic_message(translation_message_request(text, model, target_language))
                .await?;
            return translation_text_from_response(&response);
        }

        let url = api_url_with_suffix(
            &self.base_url,
            "chat/completions",
            self.path_suffix.as_deref(),
        );
        let mut body = serde_json::json!({
            "model": model,
            "messages": [
                {
                    "role": "system",
                    "content": translation_system_prompt(target_language)
                },
                {
                    "role": "user",
                    "content": text
                }
            ],
            "max_tokens": 4096,
            "temperature": 0.1,
            "stream": false
        });
        apply_reasoning_effort(&mut body, Some("off"), self.api_provider);

        let response = self
            .send_with_retry(|| self.http_client.post(&url).json(&body))
            .await?;

        let value: serde_json::Value = response.json().await?;
        let translated = value["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("translate: unexpected API response shape"))?
            .trim()
            .to_string();

        Ok(translated)
    }

    /// List available models from the provider.
    pub async fn list_models(&self) -> Result<Vec<AvailableModel>> {
        let url = api_url(&self.base_url, "models");
        let response = self.send_with_retry(|| self.http_client.get(&url)).await?;

        let status = response.status();
        if !status.is_success() {
            let raw_error_text = bounded_error_text(response, ERROR_BODY_MAX_BYTES).await;
            let error_text = sanitize_http_error_body(
                Some(self.api_provider.display_name()),
                status.as_u16(),
                &raw_error_text,
            );
            anyhow::bail!("Failed to list models: HTTP {status}: {error_text}");
        }
        let response_text = response
            .text()
            .await
            .context("Failed to read models response body")?;

        parse_models_response(&response_text)
    }

    /// The catalog provider id for this client (the `ProviderKind` slug, falling
    /// back to the `ApiProvider` slug for legacy variants without a kind). This
    /// is the id used as the cache scope and `CatalogOffering.provider`.
    fn catalog_provider_id(&self) -> String {
        self.api_provider
            .kind()
            .map(|kind| kind.as_str().to_string())
            .unwrap_or_else(|| self.api_provider.as_str().to_string())
    }

    /// Fetch the provider's live `/models` listing as a secret-free
    /// [`ProviderCatalogDelta`] (#3385).
    ///
    /// Uses the same URL construction and auth client as [`Self::list_models`],
    /// but issues a single request without `send_with_retry` so a refresh
    /// failure stays typed and non-fatal — bundled / saved / static rows are
    /// untouched. The delta is scoped to the base-URL fingerprint and stamped
    /// with the fetch time; the API key authorizes the request but is **never**
    /// persisted into the delta or cache. Unknown live rows carry no canonical
    /// model, capabilities, or pricing, per the #3385 contract.
    pub async fn fetch_catalog_delta(&self) -> Result<ProviderCatalogDelta, CatalogRefreshError> {
        let url = api_url(&self.base_url, "models");
        // A catalog refresh is non-fatal and must produce a *typed* outcome, so
        // it issues a single request and maps the raw status. This intentionally
        // does NOT route through `send_with_retry` like `list_models` does: that
        // path erases the HTTP status into a generic error and retries
        // non-retryable auth failures, neither of which suits a typed refresh.
        // Auth headers are baked into `http_client` (the key is used but never
        // persisted into the delta or cache).
        let response = self
            .http_client
            .get(&url)
            .send()
            .await
            .map_err(|_| CatalogRefreshError::Network)?;

        let status = response.status();
        if !status.is_success() {
            return Err(match status.as_u16() {
                401 => CatalogRefreshError::Unauthorized,
                403 => CatalogRefreshError::Forbidden,
                404 => CatalogRefreshError::NotFound,
                429 => CatalogRefreshError::RateLimited,
                // Any other non-success (5xx, unexpected) is treated as a
                // transient transport-class failure.
                _ => CatalogRefreshError::Network,
            });
        }

        let body = response
            .text()
            .await
            .map_err(|_| CatalogRefreshError::Network)?;
        let models =
            parse_models_response(&body).map_err(|_| CatalogRefreshError::InvalidResponse)?;
        if models.is_empty() {
            return Err(CatalogRefreshError::EmptyList);
        }

        let provider = self.catalog_provider_id();
        let fingerprint = base_url_fingerprint(&self.base_url);
        let fetched_at = now_unix();
        let offerings = models
            .into_iter()
            .map(|model| CatalogOffering {
                provider: provider.clone(),
                wire_model_id: model.id,
                canonical_model: None,
                // This refresh calls the chat-model listing endpoint. A future
                // provider-specific catalog adapter can split image/TTS/embed
                // rows before they become executable route candidates.
                endpoint_key: "chat".to_string(),
                default_for_provider: false,
                family: None,
                limit: None,
                cost: None,
                reasoning: None,
                reasoning_options: Vec::new(),
                source: CatalogSource::Live {
                    base_url_fingerprint: fingerprint.clone(),
                    fetched_at,
                },
            })
            .collect();

        Ok(ProviderCatalogDelta {
            provider,
            base_url_fingerprint: fingerprint,
            fetched_at,
            offerings,
        })
    }

    /// Refresh `cache` for this client's provider + base URL, recording either a
    /// success or a typed failure (#3385). Returns the resulting status so the UI
    /// can surface a visible "fresh / failed(reason)" chip without inspecting the
    /// cache internals. A failed refresh preserves any previously cached rows.
    pub async fn refresh_catalog_cache(
        &self,
        cache: &mut ProviderCatalogCache,
        ttl_secs: u64,
    ) -> CatalogStatus {
        match self.fetch_catalog_delta().await {
            Ok(delta) => {
                cache.record_success(delta, ttl_secs);
                CatalogStatus::Fresh
            }
            Err(reason) => {
                cache.record_failure(
                    &self.catalog_provider_id(),
                    &base_url_fingerprint(&self.base_url),
                    reason,
                );
                CatalogStatus::Failed { reason }
            }
        }
    }

    /// Generate speech with Xiaomi MiMo TTS models.
    ///
    /// The spoken text is placed in an `assistant` message because Xiaomi
    /// MiMo's TTS chat-completions surface expects that shape. The optional
    /// `instruction` is a `user` message that controls style, voice design, or
    /// voice-clone performance and is not spoken verbatim.
    pub async fn synthesize_speech(
        &self,
        request: SpeechSynthesisRequest,
    ) -> Result<SpeechSynthesisResponse> {
        if self.api_provider != crate::config::ApiProvider::XiaomiMimo {
            anyhow::bail!(
                "speech synthesis requires provider 'xiaomi-mimo' (current: {})",
                self.api_provider.as_str()
            );
        }

        let model = request.model.trim().to_string();
        if model.is_empty() {
            anyhow::bail!("Speech model cannot be empty");
        }
        let text = request.text.trim().to_string();
        if text.is_empty() {
            anyhow::bail!("Speech text cannot be empty");
        }

        let audio_format = normalize_audio_format(&request.audio_format);
        let model = wire_model_for_provider(self.api_provider, &model);
        let model_lower = model.to_ascii_lowercase();
        let instruction = request
            .instruction
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let voice = request
            .voice
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);

        if model_lower.contains("voicedesign") && instruction.is_none() {
            anyhow::bail!(
                "Model '{model}' requires a voice design prompt. Pass --voice-prompt or --instruction."
            );
        }
        if model_lower.contains("voiceclone") && voice.is_none() {
            anyhow::bail!(
                "Model '{model}' requires cloned voice data. Pass --clone-voice <mp3|wav> or --voice <data-uri>."
            );
        }

        let mut audio = json!({
            "format": audio_format.clone(),
        });
        if let Some(voice) = voice.as_deref() {
            audio["voice"] = json!(voice);
        }

        let body = build_speech_synthesis_body(&model, &text, instruction, audio);

        let url = api_url(&self.base_url, "chat/completions");
        let response = self
            .send_with_retry(|| self.http_client.post(&url).json(&body))
            .await?;
        let status = response.status();
        if !status.is_success() {
            let raw_error_text = bounded_error_text(response, ERROR_BODY_MAX_BYTES).await;
            let error_text = sanitize_http_error_body(
                Some(self.api_provider.display_name()),
                status.as_u16(),
                &raw_error_text,
            );
            anyhow::bail!("Speech synthesis failed: HTTP {status}: {error_text}");
        }

        let response_text = response
            .text()
            .await
            .context("Failed to read speech synthesis response body")?;
        let payload: Value = serde_json::from_str(&response_text)
            .context("Failed to parse speech synthesis response JSON")?;
        let (audio_bytes, transcript) = parse_speech_audio_response(&payload)?;

        Ok(SpeechSynthesisResponse {
            model,
            audio_format,
            audio_bytes,
            transcript,
            voice,
        })
    }

    async fn wait_for_rate_limit(&self) {
        let maybe_delay = {
            let mut limiter = self.rate_limiter.lock().await;
            limiter.delay_until_available(1.0)
        };
        if let Some(delay) = maybe_delay {
            tokio::time::sleep(delay).await;
        }
    }

    async fn mark_request_success(&self) {
        let mut health = self.connection_health.lock().await;
        if apply_request_success(&mut health, Instant::now()) {
            logging::info("Connection recovered");
        }
    }

    async fn mark_request_failure(&self, reason: &str) {
        let mut health = self.connection_health.lock().await;
        apply_request_failure(&mut health, Instant::now());
        logging::warn(format!(
            "Connection degraded (failures={}): {}",
            health.consecutive_failures, reason
        ));
    }

    async fn maybe_probe_recovery(&self) {
        let should_probe = {
            let mut health = self.connection_health.lock().await;
            mark_recovery_probe_if_due(&mut health, Instant::now())
        };
        if !should_probe {
            return;
        }
        if api_provider_skips_models_probe(self.api_provider) {
            self.mark_request_success().await;
            logging::info("Skipping /models recovery probe for provider without a models endpoint");
            return;
        }
        let health_url = api_url(&self.base_url, "models");
        let probe = self.http_client.get(health_url).send().await;
        match probe {
            Ok(resp) if resp.status().is_success() => {
                // Consume the response body so the connection can be returned to the pool.
                let _ = resp.text().await;
                self.mark_request_success().await;
                logging::info("Recovery probe succeeded");
            }
            Ok(resp) => {
                self.mark_request_failure(&format!("probe status={}", resp.status()))
                    .await;
            }
            Err(err) => {
                self.mark_request_failure(&format!("probe error={err}"))
                    .await;
            }
        }
    }

    pub(super) async fn send_with_retry<F>(&self, mut build: F) -> Result<reqwest::Response>
    where
        F: FnMut() -> reqwest::RequestBuilder,
    {
        let retry_cfg: LlmRetryConfig = self.retry.clone().into();
        let request_result = with_retry(
            &retry_cfg,
            || {
                let request = build();
                async move {
                    while let Some(delay) = crate::retry_status::rate_limit_remaining() {
                        tokio::time::sleep(delay).await;
                    }
                    self.wait_for_rate_limit().await;
                    let response = request
                        .send()
                        .await
                        .map_err(|err| LlmError::from_reqwest(&err))?;
                    let status = response.status();
                    if status.is_success() {
                        return Ok(response);
                    }
                    let retry_after = extract_retry_after(response.headers());
                    let body = bounded_error_text(response, ERROR_BODY_MAX_BYTES).await;
                    let body = sanitize_http_error_body(
                        Some(self.api_provider.display_name()),
                        status.as_u16(),
                        &body,
                    );
                    Err(LlmError::from_http_response_with_retry_after(
                        status.as_u16(),
                        &body,
                        retry_after,
                    ))
                }
            },
            Some(Box::new(|err, attempt, delay| {
                let (reason_label, human_reason) = retry_reason_label_and_human(err);
                logging::warn(format!(
                    "HTTP retry reason={} attempt={} delay={:.2}s",
                    reason_label,
                    attempt + 1,
                    delay.as_secs_f64(),
                ));
                if matches!(err, LlmError::RateLimited { .. }) {
                    crate::retry_status::note_rate_limit(delay);
                }
                crate::retry_status::start(attempt + 1, delay, human_reason);
            })),
        )
        .await;

        match request_result {
            Ok(response) => {
                crate::retry_status::succeeded();
                self.mark_request_success().await;
                Ok(response)
            }
            Err(err) => {
                if let LlmError::RateLimited { retry_after, .. } = &err.last_error {
                    crate::retry_status::note_rate_limit(
                        retry_after
                            .unwrap_or_else(|| retry_cfg.delay_for_attempt(retry_cfg.max_retries)),
                    );
                }
                let last = err.last_error.to_string();
                if err.attempts > 1 {
                    crate::retry_status::failed(last.clone());
                } else {
                    crate::retry_status::clear();
                }
                self.mark_request_failure(&last).await;
                self.maybe_probe_recovery().await;
                Err(anyhow::anyhow!(last))
            }
        }
    }
}

/// Translate the structured `LlmError` into both a categorical label
/// (for structured logs / metrics) and a short human reason string
/// (for the retry banner). Returning both from one match avoids the
/// double-classification we had before.
fn retry_reason_label_and_human(err: &LlmError) -> (&'static str, String) {
    match err {
        LlmError::RateLimited { retry_after, .. } => {
            let human = if let Some(after) = retry_after {
                format!("rate limited (Retry-After {}s)", after.as_secs())
            } else {
                "rate limited".to_string()
            };
            ("rate_limited", human)
        }
        LlmError::ServerError { status, .. } => ("server_error", format!("upstream {status}")),
        LlmError::NetworkError(_) => ("network_error", "network error".to_string()),
        LlmError::Timeout(_) => ("timeout", "timeout".to_string()),
        _ => ("other", "other".to_string()),
    }
}

impl LlmClient for DeepSeekClient {
    fn provider_name(&self) -> &'static str {
        self.api_provider.as_str()
    }

    fn model(&self) -> &str {
        &self.default_model
    }

    async fn health_check(&self) -> Result<bool> {
        if api_provider_skips_models_probe(self.api_provider) {
            self.mark_request_success().await;
            return Ok(true);
        }
        let health_url = api_url(&self.base_url, "models");
        self.wait_for_rate_limit().await;
        let response = self.http_client.get(health_url).send().await;
        match response {
            Ok(resp) if resp.status().is_success() => {
                // Consume the response body so the connection can be returned to the pool.
                let _ = resp.text().await;
                self.mark_request_success().await;
                Ok(true)
            }
            Ok(resp) => {
                self.mark_request_failure(&format!("health status={}", resp.status()))
                    .await;
                Ok(false)
            }
            Err(err) => {
                self.mark_request_failure(&format!("health error={err}"))
                    .await;
                Ok(false)
            }
        }
    }

    async fn create_message(&self, request: MessageRequest) -> Result<MessageResponse> {
        if self.api_provider == ApiProvider::XiaomiMimo {
            return self.handle_responses_message(request).await;
        }
        if api_provider_uses_anthropic_messages(self.api_provider) {
            return self.handle_anthropic_message(request).await;
        }
        self.create_message_chat(&request).await
    }

    async fn create_message_stream(
        &self,
        request: MessageRequest,
    ) -> Result<crate::llm_client::StreamEventBox> {
        if self.api_provider == ApiProvider::XiaomiMimo {
            return self.handle_responses_stream(request).await;
        }
        if api_provider_uses_anthropic_messages(self.api_provider) {
            return self.handle_anthropic_stream(request).await;
        }
        self.handle_chat_completion_stream(request).await
    }
}

#[derive(Debug, Deserialize)]
struct ModelsListResponse {
    data: Vec<ModelListItem>,
}

#[derive(Debug, Deserialize)]
struct ModelListItem {
    id: String,
    #[serde(default)]
    owned_by: Option<String>,
    #[serde(default)]
    created: Option<u64>,
}

pub(super) fn parse_models_response(payload: &str) -> Result<Vec<AvailableModel>> {
    let parsed: ModelsListResponse =
        serde_json::from_str(payload).context("Failed to parse model list JSON")?;

    let mut models = parsed
        .data
        .into_iter()
        .map(|item| AvailableModel {
            id: item.id,
            owned_by: item.owned_by,
            created: item.created,
        })
        .collect::<Vec<_>>();
    models.sort_by(|a, b| a.id.cmp(&b.id));
    models.dedup_by(|a, b| a.id == b.id);
    Ok(models)
}

pub(super) fn system_to_instructions(system: Option<SystemPrompt>) -> Option<String> {
    match system {
        Some(SystemPrompt::Text(text)) => Some(text),
        Some(SystemPrompt::Blocks(blocks)) => {
            let joined = blocks
                .into_iter()
                .map(|b| b.text)
                .collect::<Vec<_>>()
                .join("\n\n---\n\n");
            if joined.trim().is_empty() {
                None
            } else {
                Some(joined)
            }
        }
        None => None,
    }
}

pub(super) fn apply_reasoning_effort(
    body: &mut Value,
    effort: Option<&str>,
    provider: ApiProvider,
) {
    if matches!(provider, ApiProvider::XiaomiMimo) {
        // MiniMax's OpenAI-compatible API keeps thinking inside `content`
        // unless reasoning_split is enabled. Always request the split shape
        // so private thinking renders as Thinking cells rather than answer
        // prose.
        body["reasoning_split"] = json!(true);
    }
    let Some(effort) = effort else {
        return;
    };
    let normalized = effort.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "off" | "disabled" | "none" | "false" => match provider {
            ApiProvider::XiaomiMimo => {
                body["thinking"] = json!({ "type": "disabled" });
            }
            ApiProvider::Custom => {}
        },
        "low" | "minimal" | "medium" | "mid" | "high" | "" => match provider {
            ApiProvider::XiaomiMimo => {
                let value = match normalized.as_str() {
                    "low" | "minimal" => "low",
                    "medium" | "mid" => "medium",
                    _ => "high",
                };
                body["reasoning_effort"] = json!(value);
                body["thinking"] = json!({ "type": "enabled" });
            }
            ApiProvider::Custom => {}
        },
        "xhigh" | "max" | "highest" | "ultracode" => match provider {
            ApiProvider::XiaomiMimo => {
                body["reasoning_effort"] = json!("max");
                body["thinking"] = json!({ "type": "enabled" });
            }
            ApiProvider::Custom => {}
        },
        _ => {}
    }
}

pub(super) fn parse_usage(usage: Option<&Value>) -> Usage {
    let input_tokens = usage
        .and_then(|u| u.get("input_tokens").or_else(|| u.get("prompt_tokens")))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let mut output_tokens = usage
        .and_then(|u| {
            u.get("output_tokens")
                .or_else(|| u.get("completion_tokens"))
        })
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let total_tokens = usage
        .and_then(|u| u.get("total_tokens"))
        .and_then(Value::as_u64);
    let reasoning_tokens_raw = usage
        .and_then(|u| u.get("completion_tokens_details"))
        .and_then(|details| details.get("reasoning_tokens"))
        .and_then(Value::as_u64);
    if output_tokens == 0
        && let Some(reasoning_tokens) = reasoning_tokens_raw
    {
        output_tokens = reasoning_tokens;
    } else if output_tokens == 0
        && let Some(total_tokens) = total_tokens
    {
        output_tokens = total_tokens.saturating_sub(input_tokens);
    }
    let cached_tokens = usage
        .and_then(|u| u.get("prompt_tokens_details"))
        .and_then(|details| details.get("cached_tokens"))
        .and_then(Value::as_u64);
    let prompt_cache_hit_tokens = usage
        .and_then(|u| u.get("prompt_cache_hit_tokens"))
        .and_then(Value::as_u64)
        .or(cached_tokens)
        .map(|v| v as u32);
    let prompt_cache_miss_tokens = usage
        .and_then(|u| u.get("prompt_cache_miss_tokens"))
        .and_then(Value::as_u64)
        .or_else(|| prompt_cache_hit_tokens.map(|hit| input_tokens.saturating_sub(u64::from(hit))))
        .map(|v| v as u32);
    let reasoning_tokens = reasoning_tokens_raw.map(|v| v as u32);

    let server_tool_use = usage.and_then(|u| u.get("server_tool_use")).map(|server| {
        let code_execution_requests = server
            .get("code_execution_requests")
            .and_then(Value::as_u64)
            .map(|v| v as u32);
        let tool_search_requests = server
            .get("tool_search_requests")
            .and_then(Value::as_u64)
            .map(|v| v as u32);
        ServerToolUsage {
            code_execution_requests,
            tool_search_requests,
        }
    });

    Usage {
        input_tokens: input_tokens.min(u64::from(u32::MAX)) as u32,
        output_tokens: output_tokens.min(u64::from(u32::MAX)) as u32,
        prompt_cache_hit_tokens,
        prompt_cache_miss_tokens,
        reasoning_tokens,
        reasoning_replay_tokens: None,
        server_tool_use,
    }
}

impl DeepSeekClient {
    /// Call the DeepSeek `/beta/completions` FIM endpoint.
    pub async fn fim_completion(
        &self,
        model: &str,
        prompt: &str,
        suffix: &str,
        max_tokens: u32,
    ) -> anyhow::Result<String> {
        if api_provider_uses_anthropic_messages(self.api_provider) {
            bail!(
                "FIM completion is not supported for {} because it uses the Anthropic Messages protocol",
                self.api_provider.display_name()
            );
        }
        let url = api_url_with_suffix(&self.base_url, "beta/completions", None);
        let model = wire_model_for_provider(self.api_provider, model);
        let body = json!({
            "model": model,
            "prompt": prompt,
            "suffix": suffix,
            "max_tokens": max_tokens,
        });
        let response = self
            .send_with_retry(|| self.http_client.post(&url).json(&body))
            .await?;
        let status = response.status();
        if !status.is_success() {
            let raw_error_text = bounded_error_text(response, ERROR_BODY_MAX_BYTES).await;
            let error_text = sanitize_http_error_body(
                Some(self.api_provider.display_name()),
                status.as_u16(),
                &raw_error_text,
            );
            anyhow::bail!("FIM API error: HTTP {status}: {error_text}");
        }
        let response_text = response
            .text()
            .await
            .context("Failed to read FIM API response body")?;
        let value: serde_json::Value =
            serde_json::from_str(&response_text).context("Failed to parse FIM API response")?;
        let text = value
            .pointer("/choices/0/text")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("FIM response missing choices[0].text"))?;
        Ok(text.to_string())
    }
}

mod anthropic;
mod chat;
mod responses;

fn extract_sse_data_value(line: &str) -> Option<&str> {
    line.strip_prefix("data:")
        .map(|value| value.strip_prefix(' ').unwrap_or(value))
}

pub(crate) use chat::{CacheWarmupKey, PromptInspection};

pub(crate) fn inspect_prompt_for_request(request: &MessageRequest) -> PromptInspection {
    chat::inspect_prompt_for_request(request)
}

pub(crate) fn build_cache_warmup_request(request: &MessageRequest) -> MessageRequest {
    chat::build_cache_warmup_request(request)
}

#[cfg(test)]
mod tests {}
