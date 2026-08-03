//! Provider-neutral `/v1/chat/completions` pass-through endpoint.
//!
//! This module resolves a model through the [`ModelRegistry`], looks up the
//! matching provider configuration, and forwards an OpenAI-compatible request
//! body upstream.  It does **not** import or call any DeepSeek-named client
//! APIs — routing stays in neutral config/provider types.
//!
//! Only providers whose [`WireFormat`] is [`WireFormat::ChatCompletions`] are
//! served.  Streaming requests are explicitly rejected for now.

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderName, StatusCode};
use axum::response::IntoResponse;
use mimofan_agent::ModelRegistry;
use mimofan_config::{ConfigToml, ProviderKind, provider::WireFormat};
use serde_json::Value;

use super::AppState;

// ── Resolved endpoint ──────────────────────────────────────────────────

/// Everything needed to forward a single chat-completions request upstream.
#[derive(Debug, Clone)]
pub struct ResolvedModelEndpoint {
    pub provider: ProviderKind,
    pub base_url: String,
    pub model: String,
    pub api_key: Option<String>,
    pub http_headers: BTreeMap<String, String>,
    pub path_suffix: Option<String>,
    pub insecure_skip_tls_verify: bool,
    pub wire_format: WireFormat,
}

// ── Resolution ─────────────────────────────────────────────────────────

/// Resolve a provider endpoint from the app configuration + an optional
/// `model` field pulled out of the incoming request body.
fn resolve_endpoint(
    config: &ConfigToml,
    registry: &ModelRegistry,
    request_model: Option<&str>,
) -> ResolvedModelEndpoint {
    let provider_kind = provider_for_request(config, registry, request_model);
    let provider_cfg = config.providers.for_provider(provider_kind);
    let provider_meta = provider_kind.provider();

    // Base URL: configured → default
    let base_url = provider_cfg
        .base_url
        .clone()
        .unwrap_or_else(|| provider_meta.default_base_url().to_string());

    // Model: request → configured → provider-level configured → default
    let model = request_model
        .filter(|m| !m.trim().is_empty())
        .map(str::to_string)
        .or_else(|| provider_cfg.model.clone())
        .unwrap_or_else(|| provider_meta.default_model().to_string());

    // API key: configured → environment
    let api_key = provider_cfg.api_key.clone().or_else(|| {
        provider_meta
            .env_vars()
            .iter()
            .find_map(|var| std::env::var(var).ok())
    });

    let http_headers = provider_cfg.http_headers.clone();

    let path_suffix = provider_cfg.path_suffix.clone();

    let insecure_skip_tls_verify = provider_cfg.insecure_skip_tls_verify.unwrap_or(false);

    let wire_format = provider_meta.wire();

    ResolvedModelEndpoint {
        provider: provider_kind,
        base_url,
        model,
        api_key,
        http_headers,
        path_suffix,
        insecure_skip_tls_verify,
        wire_format,
    }
}

/// Determine which provider to use for a chat-completions request.
///
/// 1. If the request includes a `model` name, resolve it through the registry.
///    When the registry finds a match (not a fallback), use that provider.
/// 2. Otherwise fall back to the configured default provider.
fn provider_for_request(
    config: &ConfigToml,
    registry: &ModelRegistry,
    request_model: Option<&str>,
) -> ProviderKind {
    if let Some(model_name) = request_model {
        let resolved = registry.resolve(Some(model_name), None);
        // Only use the registry's provider hint when the model was actually
        // matched; otherwise the registry's fallback is noise and we should
        // respect the configured default provider.
        if !resolved.used_fallback {
            return resolved.resolved.provider;
        }
    }
    // Fall back to configured provider.
    config.provider
}

/// Build the upstream URL.
pub fn upstream_url(endpoint: &ResolvedModelEndpoint) -> String {
    let base = endpoint.base_url.trim_end_matches('/');
    match endpoint.path_suffix.as_deref() {
        Some(suffix) if !suffix.trim().is_empty() => format!(
            "{}/{}",
            unversioned_base_url(base),
            suffix.trim_start_matches('/')
        ),
        _ => {
            let mut versioned = versioned_base_url(base);
            if versioned
                .rsplit('/')
                .next()
                .is_some_and(|segment| segment.eq_ignore_ascii_case("beta"))
            {
                versioned = format!("{}/v1", unversioned_base_url(base));
            }
            format!("{}/chat/completions", versioned.trim_end_matches('/'))
        }
    }
}

fn versioned_base_url(base_url: &str) -> String {
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

// ── Route handler ──────────────────────────────────────────────────────

pub(crate) async fn chat_completions_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(mut body): Json<Value>,
) -> impl IntoResponse {
    // Reject streaming early.
    if body
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": {
                    "message": "streaming is not supported on this endpoint",
                    "type": "unsupported_parameter",
                    "code": "streaming_unsupported"
                }
            })),
        )
            .into_response();
    }

    // Extract model from body.
    let request_model = body.get("model").and_then(|v| v.as_str());

    // Resolve endpoint.
    let config = state.config.read().await;
    let endpoint = resolve_endpoint(&config, &state.registry, request_model);

    // Only ChatCompletions providers are supported.
    if endpoint.wire_format != WireFormat::ChatCompletions {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": {
                    "message": format!(
                        "provider {:?} uses {:?} wire format, only ChatCompletions is supported",
                        endpoint.provider, endpoint.wire_format
                    ),
                    "type": "unsupported_provider",
                    "code": "provider_wire_format_unsupported"
                }
            })),
        )
            .into_response();
    }

    // Inject default model if the request didn't include one.
    if request_model.is_none() || request_model.is_some_and(|m| m.trim().is_empty()) {
        body["model"] = serde_json::Value::String(endpoint.model.clone());
    }

    let url = upstream_url(&endpoint);

    if endpoint.insecure_skip_tls_verify {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": {
                    "message": format!(
                        "TLS certificate verification cannot be disabled for provider {:?}; use SSL_CERT_FILE with a trusted custom CA bundle",
                        endpoint.provider
                    ),
                    "type": "invalid_request_error",
                    "code": "tls_verification_required"
                }
            })),
        )
            .into_response();
    }

    // Build upstream request.
    let upstream_req = reqwest::Client::builder()
        .build()
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": {
                        "message": format!("failed to build upstream client: {e}"),
                        "type": "internal_error"
                    }
                })),
            )
                .into_response()
        })
        .map(|client| {
            let mut req = client.post(&url).json(&body);

            // Auth: configured API key takes priority (the proxy owns credentials).
            // Incoming Bearer header is only used as a fallback when no configured key exists.
            let auth_from_header = headers
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .and_then(|raw| raw.strip_prefix("Bearer "));
            let api_key = endpoint.api_key.as_deref().or(auth_from_header);
            if let Some(key) = api_key {
                req = req.bearer_auth(key);
            }

            // Forward configured provider headers.
            for (name, value) in &endpoint.http_headers {
                if let Ok(header_name) = HeaderName::from_bytes(name.as_bytes()) {
                    req = req.header(header_name, value.as_str());
                }
            }

            req
        });

    let client = match upstream_req {
        Ok(client) => client,
        Err(resp) => return resp,
    };

    // Execute upstream request.
    match client.send().await {
        Ok(upstream_resp) => {
            let status = upstream_resp.status();
            let headers = upstream_resp.headers().clone();
            match upstream_resp.text().await {
                Ok(body_text) => {
                    let mut response =
                        axum::response::Response::new(axum::body::Body::from(body_text));
                    *response.status_mut() = status;
                    // Forward relevant upstream headers.
                    if let Some(ct) = headers.get("content-type") {
                        response.headers_mut().insert("content-type", ct.clone());
                    }
                    response
                }
                Err(e) => (
                    StatusCode::BAD_GATEWAY,
                    Json(serde_json::json!({
                        "error": {
                            "message": format!("failed to read upstream response: {e}"),
                            "type": "upstream_error"
                        }
                    })),
                )
                    .into_response(),
            }
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({
                "error": {
                    "message": format!("upstream request failed: {e}"),
                    "type": "upstream_error"
                }
            })),
        )
            .into_response(),
    }
}

// ── Tests ──────────────────────────────────────────────────────────────
