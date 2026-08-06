// Tests relocated from src/lib.rs (issue #547 Phase 3).

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::Request;
use axum::http::{Method, StatusCode, header};
use axum::response::Response;
use mimofan_app_server::*;
use mimofan_protocol::AppRequest;
use serde_json::{Value, json};
use std::fs;
use tower::ServiceExt;

fn app_with_config(auth_token: Option<&str>) -> (Router, tempfile::TempDir) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let config_path = tmp.path().join("config.toml");
    fs::write(&config_path, "api_key = \"sk-deepseek-secret\"\n").expect("write config");
    let state = build_state(
        Some(config_path),
        auth_token.map(std::string::ToString::to_string),
    )
    .expect("state");
    (app_router(state, &[]), tmp)
}

#[test]
fn build_state_keeps_resolved_explicit_config_path() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let config_dir = tmp.path().join("config-dir");
    fs::create_dir_all(&config_dir).expect("config dir");
    let config_path = config_dir.join("config.toml");
    fs::write(&config_path, "api_key = \"sk-deepseek-secret\"\n").expect("write config");

    let state = build_state(Some(config_path.clone()), None).expect("state");

    assert_eq!(
        state.config_path.as_deref(),
        Some(
            config_path
                .canonicalize()
                .expect("canonical config")
                .as_path()
        )
    );
}

async fn response_body_json(response: Response) -> Value {
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body bytes");
    serde_json::from_slice(&bytes).expect("json response")
}

#[tokio::test]
async fn http_app_routes_require_bearer_token_when_auth_enabled() {
    let (app, _tmp) = app_with_config(Some("test-token"));
    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/app")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&AppRequest::ConfigGet {
                        key: "api_key".to_string(),
                    })
                    .expect("request json"),
                ))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn http_config_get_redacts_sensitive_values_after_auth() {
    let (app, _tmp) = app_with_config(Some("test-token"));
    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/app")
                .header(header::AUTHORIZATION, "Bearer test-token")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&AppRequest::ConfigGet {
                        key: "api_key".to_string(),
                    })
                    .expect("request json"),
                ))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_body_json(response).await;
    assert_eq!(body["data"]["value"], "sk-d***cret");
}

#[tokio::test]
async fn cors_does_not_allow_arbitrary_origins() {
    let (app, _tmp) = app_with_config(Some("test-token"));
    let response = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/healthz")
                .header(header::ORIGIN, "https://attacker.example")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .is_none()
    );
}

#[tokio::test]
async fn build_state_loads_permissions_into_runtime_policy() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let config_path = tmp.path().join("config.toml");
    fs::write(&config_path, "api_key = \"sk-deepseek-secret\"\n").expect("write config");
    fs::write(
        tmp.path().join("permissions.toml"),
        r#"
            [[rules]]
            tool = "exec_shell"
            command = "cargo test"
            "#,
    )
    .expect("write permissions");

    let state = build_state(Some(config_path), None).expect("state");
    let runtime = state.runtime.write().await;
    let decision = runtime
        .exec_policy
        .check(mimofan_execpolicy::ExecPolicyContext {
            command: "cargo test --workspace",
            cwd: "/workspace",
            tool: Some("exec_shell"),
            path: None,
            ask_for_approval: mimofan_execpolicy::AskForApproval::UnlessTrusted,
            sandbox_mode: Some("workspace-write"),
        })
        .expect("policy check");

    assert!(decision.allow);
    assert!(decision.requires_approval);
    assert_eq!(
        decision.matched_rule.as_deref(),
        Some("tool=exec_shell command=cargo test")
    );
}

#[test]
fn non_loopback_bind_without_auth_fails_fast() {
    let options = AppServerOptions {
        listen: "0.0.0.0:8787".parse().expect("socket addr"),
        config_path: None,
        auth_token: None,
        insecure_no_auth: false,
        cors_origins: Vec::new(),
    };

    let err = resolve_auth_token(&options).expect_err("non-loopback generated auth should fail");
    assert!(err.to_string().contains("without explicit auth token"));
}

#[tokio::test]
async fn stdio_transport_keeps_raw_config_get_for_legacy_clients() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let config_path = tmp.path().join("config.toml");
    fs::write(&config_path, "").expect("write config");
    let state = build_state(Some(config_path), None).expect("state");
    {
        let mut cfg = state.config.write().await;
        cfg.api_key = Some("sk-deepseek-secret".to_string());
    }

    let response = process_app_request(
        &state,
        AppRequest::ConfigGet {
            key: "api_key".to_string(),
        },
        AppTransport::Stdio,
    )
    .await;

    assert_eq!(response.data["value"], "sk-deepseek-secret");
}

#[tokio::test]
async fn stdio_thread_goal_methods_round_trip_persisted_goal() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let config_path = tmp.path().join("config.toml");
    fs::write(&config_path, "").expect("write config");
    let state = build_state(Some(config_path), None).expect("state");

    let capabilities = dispatch_stdio_request(&state, "thread/capabilities", json!({}))
        .await
        .expect("thread capabilities");
    assert!(
        capabilities.result["methods"]
            .as_array()
            .expect("methods")
            .iter()
            .any(|method| method == "thread/goal/set")
    );

    let started = dispatch_stdio_request(&state, "thread/start", json!({}))
        .await
        .expect("start thread");
    let thread_id = started.result["thread_id"]
        .as_str()
        .expect("thread id")
        .to_string();

    let set = dispatch_stdio_request(
        &state,
        "thread/goal/set",
        json!({
            "thread_id": thread_id,
            "objective": "Release 0.8.59",
            "token_budget": 59000
        }),
    )
    .await
    .expect("set goal");
    assert_eq!(set.result["status"], "ok");
    assert_eq!(set.result["goal"]["objective"], "Release 0.8.59");
    assert_eq!(set.result["goal"]["status"], "active");

    let got = dispatch_stdio_request(
        &state,
        "thread/goal/get",
        json!({
            "thread_id": thread_id
        }),
    )
    .await
    .expect("get goal");
    assert_eq!(got.result["goal"]["token_budget"], 59000);

    let cleared = dispatch_stdio_request(
        &state,
        "thread/goal/clear",
        json!({
            "thread_id": thread_id
        }),
    )
    .await
    .expect("clear goal");
    assert_eq!(cleared.result["status"], "cleared");
    assert_eq!(cleared.result["data"]["cleared"], true);
}

// ── capability drift guard ─────────────────────────────────────────
//
// The stdio `capabilities` method is the benchmark/SDK contract: external
// harnesses probe it (without spending model tokens) to learn what the
// app-server can do. Pin the advertised method set so any change forces a
// deliberate update here, in the dispatcher, and in docs/RUNTIME_API.md.

/// Methods advertised by the top-level `capabilities` probe, in order.
const EXPECTED_CAPABILITY_METHODS: &[&str] = &[
    "healthz",
    "thread/capabilities",
    "thread/request",
    "thread/create",
    "thread/start",
    "thread/resume",
    "thread/fork",
    "thread/list",
    "thread/read",
    "thread/set_name",
    "thread/goal/set",
    "thread/goal/get",
    "thread/goal/clear",
    "thread/archive",
    "thread/unarchive",
    "thread/message",
    "app/capabilities",
    "app/request",
    "app/config/get",
    "app/config/set",
    "app/config/unset",
    "app/config/list",
    "app/models",
    "app/thread_loaded_list",
    "prompt/capabilities",
    "prompt/request",
    "prompt/run",
    "shutdown",
];

fn capability_test_state() -> (AppState, tempfile::TempDir) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let config_path = tmp.path().join("config.toml");
    fs::write(&config_path, "").expect("write config");
    let state = build_state(Some(config_path), None).expect("state");
    (state, tmp)
}

#[tokio::test]
async fn capabilities_method_set_is_stable() {
    let (state, _tmp) = capability_test_state();
    let caps = dispatch_stdio_request(&state, "capabilities", json!({}))
        .await
        .expect("capabilities dispatch");
    let methods: Vec<String> = caps.result["methods"]
        .as_array()
        .expect("methods array")
        .iter()
        .map(|m| m.as_str().expect("method string").to_string())
        .collect();
    assert_eq!(
        methods, EXPECTED_CAPABILITY_METHODS,
        "app-server stdio capability set drifted; update the dispatcher, this \
             snapshot, and docs/RUNTIME_API.md together"
    );
}

#[tokio::test]
async fn every_advertised_capability_is_dispatchable() {
    let (state, _tmp) = capability_test_state();
    // Empty params: methods may fail validation (-32602), but none may report
    // method-not-found (-32601). Required fields (e.g. PromptRequest.prompt)
    // make the prompt routes fail at parse time, so no model tokens are spent.
    for method in EXPECTED_CAPABILITY_METHODS {
        if let Err(err) = dispatch_stdio_request(&state, method, json!({})).await {
            assert_ne!(
                err.code,
                JsonRpcError::method_not_found(method).code,
                "advertised capability `{method}` is not dispatchable"
            );
        }
    }
}

// ── resolve_auth_token ─────────────────────────────────────────────

#[test]
fn auth_token_empty_string_fails() {
    let options = AppServerOptions {
        listen: "127.0.0.1:0".parse().expect("addr"),
        config_path: None,
        auth_token: Some("  ".to_string()),
        insecure_no_auth: false,
        cors_origins: Vec::new(),
    };
    let err = resolve_auth_token(&options).expect_err("empty token should fail");
    assert!(err.to_string().contains("cannot be empty"));
}

#[test]
fn auth_token_generated_when_none_provided() {
    let options = AppServerOptions {
        listen: "127.0.0.1:0".parse().expect("addr"),
        config_path: None,
        auth_token: None,
        insecure_no_auth: false,
        cors_origins: Vec::new(),
    };
    let token = resolve_auth_token(&options).expect("resolve auth token");
    assert!(token.is_some());
    assert!(
        token
            .expect("auth token should be present")
            .starts_with("cwapp_")
    );
}

#[test]
fn generated_auth_status_does_not_render_token() {
    let rendered = app_server_auth_status_lines(false).join("\n");

    assert!(!rendered.contains("Authorization: Bearer"));
    assert!(rendered.contains("not printed"));
    assert!(rendered.contains("MIMOFAN_APP_SERVER_TOKEN"));
}

#[test]
fn auth_token_explicit_is_preserved() {
    let options = AppServerOptions {
        listen: "127.0.0.1:0".parse().expect("addr"),
        config_path: None,
        auth_token: Some("my-secret".to_string()),
        insecure_no_auth: false,
        cors_origins: Vec::new(),
    };
    let token = resolve_auth_token(&options).expect("resolve auth token");
    assert_eq!(token.as_deref(), Some("my-secret"));
}

#[test]
fn auth_token_explicit_allows_non_loopback_bind() {
    let options = AppServerOptions {
        listen: "0.0.0.0:8787".parse().expect("socket addr"),
        config_path: None,
        auth_token: Some("my-secret".to_string()),
        insecure_no_auth: false,
        cors_origins: Vec::new(),
    };
    let token = resolve_auth_token(&options).expect("resolve auth token");
    assert_eq!(token.as_deref(), Some("my-secret"));
}

#[test]
fn insecure_no_auth_on_loopback_returns_none() {
    let options = AppServerOptions {
        listen: "127.0.0.1:0".parse().expect("addr"),
        config_path: None,
        auth_token: None,
        insecure_no_auth: true,
        cors_origins: Vec::new(),
    };
    let token = resolve_auth_token(&options).expect("resolve auth token");
    assert!(token.is_none());
}

#[test]
fn insecure_no_auth_on_non_loopback_fails_fast() {
    let options = AppServerOptions {
        listen: "0.0.0.0:8787".parse().expect("socket addr"),
        config_path: None,
        auth_token: None,
        insecure_no_auth: true,
        cors_origins: Vec::new(),
    };

    let err = resolve_auth_token(&options).expect_err("non-loopback unauth should fail");
    assert!(
        err.to_string()
            .contains("refusing unauthenticated app-server bind")
    );
}

// ── cors_layer ─────────────────────────────────────────────────────

#[test]
fn cors_layer_includes_default_origins() {
    let layer = cors_layer(&[]);
    // Just verify it doesn't panic and creates successfully
    let _ = layer;
}

#[test]
fn cors_layer_adds_extra_origins() {
    let extras = vec!["https://example.com".to_string()];
    let layer = cors_layer(&extras);
    let _ = layer;
}

#[test]
fn cors_layer_skips_empty_origins() {
    let extras = vec!["".to_string(), "  ".to_string()];
    let layer = cors_layer(&extras);
    let _ = layer;
}

// ── JsonRpc helpers ────────────────────────────────────────────────

#[test]
fn params_or_object_returns_object_for_null() {
    let result = params_or_object(Value::Null);
    assert_eq!(result, json!({}));
}

#[test]
fn params_or_object_passthrough_for_non_null() {
    let input = json!({"key": "value"});
    let result = params_or_object(input.clone());
    assert_eq!(result, input);
}

#[test]
fn jsonrpc_result_format() {
    let result = jsonrpc_result(Some(json!(1)), json!({"ok": true}));
    assert_eq!(result["jsonrpc"], "2.0");
    assert_eq!(result["id"], 1);
    assert_eq!(result["result"]["ok"], true);
}

#[test]
fn jsonrpc_result_null_id() {
    let result = jsonrpc_result(None, json!(null));
    assert_eq!(result["id"], Value::Null);
}

#[test]
fn jsonrpc_error_format() {
    let err = jsonrpc_error(Some(json!(2)), JsonRpcError::internal("oops"));
    assert_eq!(err["jsonrpc"], "2.0");
    assert_eq!(err["id"], 2);
    assert_eq!(err["error"]["code"], -32603);
    assert_eq!(err["error"]["message"], "oops");
}

#[test]
fn jsonrpc_error_codes() {
    assert_eq!(JsonRpcError::parse_error("").code, -32700);
    assert_eq!(JsonRpcError::invalid_request("").code, -32600);
    assert_eq!(JsonRpcError::method_not_found("x").code, -32601);
    assert_eq!(JsonRpcError::invalid_params("").code, -32602);
    assert_eq!(JsonRpcError::internal("").code, -32603);
}

// ── AppServerOptions ───────────────────────────────────────────────

#[test]
fn app_server_options_debug_does_not_leak_token() {
    let options = AppServerOptions {
        listen: "127.0.0.1:8080".parse().expect("addr"),
        config_path: None,
        auth_token: Some("secret-token".to_string()),
        insecure_no_auth: false,
        cors_origins: vec!["https://example.com".to_string()],
    };
    let debug = format!("{options:?}");
    assert!(!debug.contains("secret-token"));
    assert!(debug.contains("<redacted>"));
    assert!(debug.contains("8080"));
}

// ── Default CORS origins ──────────────────────────────────────────

#[test]
fn default_cors_origins_include_common_dev_ports() {
    assert!(DEFAULT_CORS_ORIGINS.contains(&"http://localhost:3000"));
    assert!(DEFAULT_CORS_ORIGINS.contains(&"http://localhost:5173"));
    assert!(DEFAULT_CORS_ORIGINS.contains(&"tauri://localhost"));
}
