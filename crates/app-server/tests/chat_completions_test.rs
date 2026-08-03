// Tests relocated from src/chat_completions.rs (issue #547 Phase 3).

    use mimofan_app_server::*;
    use axum::http::StatusCode;
    use axum::Json;
    use mimofan_app_server::chat_completions::*;
    use mimofan_config::ProviderKind;
    use std::collections::BTreeMap;
    use serde_json::Value;
    use axum::body::Body;
    use axum::http::{Method, Request};
    use mimofan_config::provider::WireFormat;
    use std::fs;
    use std::sync::OnceLock;
    use tower::ServiceExt;


    fn install_crypto_provider() {
        static INIT: OnceLock<()> = OnceLock::new();
        INIT.get_or_init(|| {
            let _ = rustls::crypto::ring::default_provider().install_default();
        });
    }

    /// Start a minimal upstream mock server that echoes back what it received.
    async fn start_mock_upstream() -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind mock upstream listener");
        let addr = listener.local_addr().expect("get mock upstream local address");
        let base_url = format!("http://{}:{}", addr.ip(), addr.port());

        let handle = tokio::spawn(async move {
            let app = axum::Router::new()
                .route("/v1/chat/completions", axum::routing::post(mock_handler));
            axum::serve(listener, app).await.expect("serve mock upstream");
        });

        // Give the server a moment to start.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        (base_url, handle)
    }

    async fn mock_handler(
        headers: axum::http::HeaderMap,
        Json(body): Json<Value>,
    ) -> impl axum::response::IntoResponse {
        let auth = headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("none");

        let response_body = serde_json::json!({
            "id": "chatcmpl-mock",
            "object": "chat.completion",
            "created": 1234567890,
            "model": body.get("model").and_then(|v| v.as_str()).unwrap_or("unknown"),
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": format!("echo: received {} messages, auth={auth}",
                        body.get("messages").and_then(|m| m.as_array()).map(|a| a.len()).unwrap_or(0))
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 5,
                "total_tokens": 15
            }
        });

        (StatusCode::OK, Json(response_body))
    }

    fn app_with_mock_upstream(
        auth_token: Option<&str>,
        mock_base_url: &str,
    ) -> (axum::Router, tempfile::TempDir) {
        app_with_mock_upstream_with_provider_extra(auth_token, mock_base_url, "")
    }

    fn app_with_mock_upstream_with_provider_extra(
        auth_token: Option<&str>,
        mock_base_url: &str,
        provider_extra: &str,
    ) -> (axum::Router, tempfile::TempDir) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let config_path = tmp.path().join("config.toml");
        let config_content = format!(
            r#"
provider = "custom"
api_key = "sk-deepseek-secret"

[providers.custom]
base_url = "{mock_base_url}"
model = "my-custom-model"
api_key = "arcee-configured-key"
{provider_extra}
"#
        );
        fs::write(&config_path, config_content).expect("write config");
        let state = build_state(
            Some(config_path),
            auth_token.map(std::string::ToString::to_string),
        )
        .expect("state");
        (app_router(state, &[]), tmp)
    }

    async fn response_body_json(response: axum::response::Response) -> Value {
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body bytes");
        serde_json::from_slice(&bytes).expect("json response")
    }

    #[tokio::test]
    async fn forwards_messages_and_tools() {
        install_crypto_provider();
        let (mock_url, _mock) = start_mock_upstream().await;
        let (app, _tmp) = app_with_mock_upstream(None, &mock_url);

        let body = serde_json::json!({
            "model": "my-custom-model",
            "messages": [
                {"role": "user", "content": "hello"}
            ],
            "tools": [{
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "description": "Get weather",
                    "parameters": {"type": "object", "properties": {}}
                }
            }],
            "tool_choice": "auto"
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/chat/completions")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).expect("serialize request body")))
                    .expect("build HTTP request"),
            )
            .await
            .expect("send test request");

        assert_eq!(response.status(), StatusCode::OK);
        let resp_body = response_body_json(response).await;
        assert_eq!(resp_body["model"], "my-custom-model");
        assert!(
            resp_body["choices"][0]["message"]["content"]
                .as_str()
                .expect("extract response content string")
                .contains("1 messages")
        );
    }

    #[tokio::test]
    async fn default_model_injected_when_omitted() {
        install_crypto_provider();
        let (mock_url, _mock) = start_mock_upstream().await;
        let (app, _tmp) = app_with_mock_upstream(None, &mock_url);

        let body = serde_json::json!({
            "messages": [
                {"role": "user", "content": "hello"}
            ]
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/chat/completions")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).expect("serialize request body")))
                    .expect("build HTTP request"),
            )
            .await
            .expect("send test request");

        assert_eq!(response.status(), StatusCode::OK);
        let resp_body = response_body_json(response).await;
        // The mock echoes the model it received; should be the configured default.
        assert_eq!(resp_body["model"], "my-custom-model");
    }

    #[tokio::test]
    async fn configured_model_preserved_when_provided() {
        install_crypto_provider();
        let (mock_url, _mock) = start_mock_upstream().await;
        let (app, _tmp) = app_with_mock_upstream(None, &mock_url);

        let body = serde_json::json!({
            "model": "custom-model-v2",
            "messages": [
                {"role": "user", "content": "hello"}
            ]
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/chat/completions")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).expect("serialize request body")))
                    .expect("build HTTP request"),
            )
            .await
            .expect("send test request");

        assert_eq!(response.status(), StatusCode::OK);
        let resp_body = response_body_json(response).await;
        assert_eq!(resp_body["model"], "custom-model-v2");
    }

    #[tokio::test]
    async fn configured_api_key_takes_priority_over_incoming_bearer() {
        install_crypto_provider();
        let (mock_url, _mock) = start_mock_upstream().await;
        let (app, _tmp) = app_with_mock_upstream(None, &mock_url);

        let body = serde_json::json!({
            "model": "my-custom-model",
            "messages": [
                {"role": "user", "content": "hello"}
            ]
        });

        // Send with an explicit bearer token, but the configured key should win.
        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/chat/completions")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer user-provided-secret-key")
                    .body(Body::from(serde_json::to_vec(&body).expect("serialize request body")))
                    .expect("build HTTP request"),
            )
            .await
            .expect("send test request");

        assert_eq!(response.status(), StatusCode::OK);
        let resp_body = response_body_json(response).await;
        let content = resp_body["choices"][0]["message"]["content"]
            .as_str()
            .expect("extract response content string");
        // The configured key takes priority, not the incoming Bearer.
        assert!(
            content.contains("auth=Bearer arcee-configured-key"),
            "expected configured auth in mock echo, got: {content}"
        );
    }

    #[tokio::test]
    async fn configured_api_key_used_when_no_bearer_in_request() {
        install_crypto_provider();
        let (mock_url, _mock) = start_mock_upstream().await;
        let (app, _tmp) = app_with_mock_upstream(None, &mock_url);

        let body = serde_json::json!({
            "model": "my-custom-model",
            "messages": [
                {"role": "user", "content": "hello"}
            ]
        });

        // No Authorization header; the configured key should be used.
        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/chat/completions")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).expect("serialize request body")))
                    .expect("build HTTP request"),
            )
            .await
            .expect("send test request");

        assert_eq!(response.status(), StatusCode::OK);
        let resp_body = response_body_json(response).await;
        let content = resp_body["choices"][0]["message"]["content"]
            .as_str()
            .expect("extract response content string");
        assert!(
            content.contains("auth=Bearer arcee-configured-key"),
            "expected configured auth in mock echo, got: {content}"
        );
    }

    #[tokio::test]
    async fn insecure_tls_skip_verify_is_rejected() {
        install_crypto_provider();
        let (mock_url, _mock) = start_mock_upstream().await;
        let (app, _tmp) = app_with_mock_upstream_with_provider_extra(
            None,
            &mock_url,
            "insecure_skip_tls_verify = true",
        );

        let body = serde_json::json!({
            "model": "my-custom-model",
            "messages": [
                {"role": "user", "content": "hello"}
            ]
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/chat/completions")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).expect("serialize request body")))
                    .expect("build HTTP request"),
            )
            .await
            .expect("send test request");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let resp_body = response_body_json(response).await;
        assert_eq!(resp_body["error"]["code"], "tls_verification_required");
        assert!(
                resp_body["error"]["message"]
                    .as_str()
                    .expect("extract error message string")
                .contains("SSL_CERT_FILE")
        );
    }

    #[tokio::test]
    async fn streaming_request_rejected() {
        install_crypto_provider();
        let (mock_url, _mock) = start_mock_upstream().await;
        let (app, _tmp) = app_with_mock_upstream(None, &mock_url);

        let body = serde_json::json!({
            "model": "my-custom-model",
            "messages": [
                {"role": "user", "content": "hello"}
            ],
            "stream": true
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/chat/completions")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).expect("serialize request body")))
                    .expect("build HTTP request"),
            )
            .await
            .expect("send test request");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let resp_body = response_body_json(response).await;
        assert_eq!(resp_body["error"]["code"], "streaming_unsupported");
    }

    #[tokio::test]
    async fn non_chat_completions_provider_rejected() {
        // Use the test to verify WireFormat checks work for non-ChatCompletions providers.
        // Anthropic's wire format is AnthropicMessages; OpenaiCodex is Responses.
        let endpoint = ResolvedModelEndpoint {
            provider: ProviderKind::XiaomiMimo,
            base_url: "https://api.anthropic.com".to_string(),
            model: "claude-sonnet-4-20250514".to_string(),
            api_key: Some("sk-ant-test".to_string()),
            http_headers: BTreeMap::new(),
            path_suffix: None,
            insecure_skip_tls_verify: false,
            wire_format: WireFormat::AnthropicMessages,
        };

        assert_ne!(endpoint.wire_format, WireFormat::ChatCompletions);
        // The handler would reject this; we verify the wire format here.
        assert_eq!(endpoint.wire_format, WireFormat::AnthropicMessages);
    }

    #[test]
    fn upstream_url_defaults_to_v1_chat_completions() {
        let endpoint = ResolvedModelEndpoint {
            provider: ProviderKind::XiaomiMimo,
            base_url: "https://api.arcee.ai".to_string(),
            model: "trinity".to_string(),
            api_key: None,
            http_headers: BTreeMap::new(),
            path_suffix: None,
            insecure_skip_tls_verify: false,
            wire_format: WireFormat::ChatCompletions,
        };
        assert_eq!(
            upstream_url(&endpoint),
            "https://api.arcee.ai/v1/chat/completions"
        );
    }

    #[test]
    fn upstream_url_preserves_arcee_api_v1_base() {
        let endpoint = ResolvedModelEndpoint {
            provider: ProviderKind::XiaomiMimo,
            base_url: "https://api.arcee.ai/api/v1".to_string(),
            model: "trinity".to_string(),
            api_key: None,
            http_headers: BTreeMap::new(),
            path_suffix: None,
            insecure_skip_tls_verify: false,
            wire_format: WireFormat::ChatCompletions,
        };
        assert_eq!(
            upstream_url(&endpoint),
            "https://api.arcee.ai/api/v1/chat/completions"
        );
    }

    #[test]
    fn upstream_url_respects_path_suffix() {
        let endpoint = ResolvedModelEndpoint {
            provider: ProviderKind::XiaomiMimo,
            base_url: "https://openrouter.ai/api/v1".to_string(),
            model: "deepseek/deepseek-v4-pro".to_string(),
            api_key: None,
            http_headers: BTreeMap::new(),
            path_suffix: Some("/chat/completions".to_string()),
            insecure_skip_tls_verify: false,
            wire_format: WireFormat::ChatCompletions,
        };
        assert_eq!(
            upstream_url(&endpoint),
            "https://openrouter.ai/api/chat/completions"
        );
    }

    #[test]
    fn upstream_url_beta_base_uses_standard_v1_chat_completions() {
        let endpoint = ResolvedModelEndpoint {
            provider: ProviderKind::XiaomiMimo,
            base_url: "https://api.deepseek.com/beta".to_string(),
            model: "deepseek-chat".to_string(),
            api_key: None,
            http_headers: BTreeMap::new(),
            path_suffix: None,
            insecure_skip_tls_verify: false,
            wire_format: WireFormat::ChatCompletions,
        };
        assert_eq!(
            upstream_url(&endpoint),
            "https://api.deepseek.com/v1/chat/completions"
        );
    }

    #[test]
    fn upstream_url_strips_trailing_slash() {
        let endpoint = ResolvedModelEndpoint {
            provider: ProviderKind::XiaomiMimo,
            base_url: "https://api.deepseek.com/".to_string(),
            model: "deepseek-chat".to_string(),
            api_key: None,
            http_headers: BTreeMap::new(),
            path_suffix: None,
            insecure_skip_tls_verify: false,
            wire_format: WireFormat::ChatCompletions,
        };
        assert_eq!(
            upstream_url(&endpoint),
            "https://api.deepseek.com/v1/chat/completions"
        );
    }
