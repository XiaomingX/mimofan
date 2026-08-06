use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Result, bail};
use axum::extract::{Request, State};
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use mimofan_agent::ModelRegistry;
use mimofan_config::{CliRuntimeOverrides, ConfigStore};
use mimofan_core::Runtime;
use mimofan_hooks::{HookDispatcher, JsonlHookSink, StdoutHookSink, UnixSocketHookSink};
use mimofan_mcp::McpManager;
use mimofan_protocol::{
    AppRequest, AppResponse, PromptRequest, PromptResponse, ThreadGoalClearParams,
    ThreadGoalGetParams, ThreadGoalSetParams, ThreadRequest, ThreadResponse,
};
use mimofan_state::StateStore;
use mimofan_tools::{ToolCall, ToolRegistry};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::RwLock;
use tower_http::cors::CorsLayer;
use uuid::Uuid;

pub mod chat_completions;

pub const DEFAULT_CORS_ORIGINS: &[&str] = &[
    "http://localhost",
    "http://localhost:1420",
    "http://localhost:3000",
    "http://localhost:5173",
    "http://127.0.0.1",
    "http://127.0.0.1:1420",
    "tauri://localhost",
];

#[derive(Clone)]
pub struct AppServerOptions {
    pub listen: SocketAddr,
    pub config_path: Option<PathBuf>,
    pub auth_token: Option<String>,
    pub insecure_no_auth: bool,
    pub cors_origins: Vec<String>,
}

impl std::fmt::Debug for AppServerOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppServerOptions")
            .field("listen", &self.listen)
            .field("config_path", &self.config_path)
            .field(
                "auth_token",
                &self.auth_token.as_ref().map(|_| "<redacted>"),
            )
            .field("insecure_no_auth", &self.insecure_no_auth)
            .field("cors_origins", &self.cors_origins)
            .finish()
    }
}

#[derive(Clone)]
pub struct AppState {
    pub config_path: Option<PathBuf>,
    pub config: Arc<RwLock<mimofan_config::ConfigToml>>,
    /// `Runtime` 内部同时存在 `&mut self` 方法（`handle_thread`/`handle_prompt`，需独占写锁）
    /// 与 `&self` 方法（`invoke_tool`/`app_status`/`mcp_startup`，可并发读锁）。
    /// 用 `RwLock` 替代原 `Mutex`：读锁路径（工具调用/任务状态/MCP 启动）可并发执行，
    /// 消除单一长任务对所有 HTTP 请求的队头阻塞（见 ARCHITECTURE_STABILITY.md §8.1）。
    pub runtime: Arc<RwLock<Runtime>>,
    registry: ModelRegistry,
    auth_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ToolCallRequest {
    call: ToolCall,
    #[serde(default)]
    cwd: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    #[serde(default)]
    jsonrpc: Option<String>,
    #[serde(default)]
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug)]
pub struct JsonRpcError {
    pub code: i64,
    message: String,
    data: Option<Value>,
}

#[derive(Debug)]
pub struct StdioDispatchResult {
    pub result: Value,
    should_exit: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppTransport {
    Http,
    Stdio,
}

#[derive(Debug, Deserialize)]
struct ConfigGetParams {
    key: String,
}

#[derive(Debug, Deserialize)]
struct ConfigSetParams {
    key: String,
    value: String,
}

#[derive(Debug, Deserialize)]
struct ThreadIdParams {
    thread_id: String,
}

#[derive(Debug, Deserialize)]
struct ThreadMessageParams {
    thread_id: String,
    input: String,
}

pub async fn run(options: AppServerOptions) -> Result<()> {
    let auth_token = resolve_auth_token(&options)?;
    let state = build_state(options.config_path.clone(), auth_token)?;
    let app = app_router(state, &options.cors_origins);

    let listener = tokio::net::TcpListener::bind(options.listen).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

pub fn app_router(state: AppState, cors_origins: &[String]) -> Router {
    let protected_routes = Router::new()
        .route("/thread", post(thread_handler))
        .route("/app", post(app_handler))
        .route("/prompt", post(prompt_handler))
        .route("/tool", post(tool_handler))
        .route("/jobs", get(jobs_handler))
        .route("/mcp/startup", post(mcp_startup_handler))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            require_app_server_token,
        ));

    Router::new()
        .route("/healthz", get(healthz))
        .route(
            "/v1/chat/completions",
            post(chat_completions::chat_completions_handler),
        )
        .merge(protected_routes)
        .layer(cors_layer(cors_origins))
        .with_state(state)
}

pub async fn run_stdio(config_path: Option<PathBuf>) -> Result<()> {
    let state = build_state(config_path, None)?;
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let mut reader = BufReader::new(stdin).lines();
    let mut writer = tokio::io::BufWriter::new(stdout);
    while let Some(line) = reader.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }

        let request: JsonRpcRequest = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(err) => {
                let response = jsonrpc_error(
                    None,
                    JsonRpcError::parse_error(format!("invalid json: {err}")),
                );
                writer.write_all(response.to_string().as_bytes()).await?;
                writer.write_all(b"\n").await?;
                writer.flush().await?;
                continue;
            }
        };

        if request
            .jsonrpc
            .as_deref()
            .is_some_and(|version| version != "2.0")
        {
            let response = jsonrpc_error(
                request.id,
                JsonRpcError::invalid_request("jsonrpc version must be 2.0"),
            );
            writer.write_all(response.to_string().as_bytes()).await?;
            writer.write_all(b"\n").await?;
            writer.flush().await?;
            continue;
        }

        let response = match dispatch_stdio_request(&state, &request.method, request.params).await {
            Ok(dispatch) => {
                let encoded = jsonrpc_result(request.id, dispatch.result);
                writer.write_all(encoded.to_string().as_bytes()).await?;
                writer.write_all(b"\n").await?;
                writer.flush().await?;
                if dispatch.should_exit {
                    break;
                }
                continue;
            }
            Err(err) => jsonrpc_error(request.id, err),
        };

        writer.write_all(response.to_string().as_bytes()).await?;
        writer.write_all(b"\n").await?;
        writer.flush().await?;
    }

    Ok(())
}

async fn healthz() -> Json<Value> {
    Json(json!({
        "status": "ok",
        "protocol": "v2",
        "service": "deepseek-app-server"
    }))
}

async fn thread_handler(
    State(state): State<AppState>,
    Json(req): Json<ThreadRequest>,
) -> Json<ThreadResponse> {
    let mut runtime = state.runtime.write().await;
    match runtime.handle_thread(req).await {
        Ok(res) => Json(res),
        Err(err) => Json(ThreadResponse {
            thread_id: "error".to_string(),
            status: format!("error:{err}"),
            thread: None,
            threads: Vec::new(),
            goal: None,
            model: None,
            model_provider: None,
            cwd: None,
            approval_policy: None,
            sandbox: None,
            events: Vec::new(),
            data: json!({}),
        }),
    }
}

async fn prompt_handler(
    State(state): State<AppState>,
    Json(req): Json<PromptRequest>,
) -> Json<PromptResponse> {
    let mut runtime = state.runtime.write().await;
    let overrides = CliRuntimeOverrides::default();
    match runtime.handle_prompt(req, &overrides).await {
        Ok(res) => Json(res),
        Err(err) => Json(PromptResponse {
            output: err.to_string(),
            model: "unknown".to_string(),
            events: Vec::new(),
        }),
    }
}

async fn tool_handler(
    State(state): State<AppState>,
    Json(req): Json<ToolCallRequest>,
) -> Json<Value> {
    let runtime = state.runtime.read().await;
    let cwd = req
        .cwd
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    // Resolve approval policy from config instead of hardcoding.
    let approval_mode = {
        let cfg = state.config.read().await;
        cfg.approval_policy
            .as_deref()
            .and_then(|p| match p.trim().to_ascii_lowercase().as_str() {
                "auto" | "yolo" => Some(mimofan_execpolicy::AskForApproval::UnlessTrusted),
                "never" | "deny" => Some(mimofan_execpolicy::AskForApproval::Never),
                _ => None,
            })
            .unwrap_or(mimofan_execpolicy::AskForApproval::OnRequest)
    };
    match runtime.invoke_tool(req.call, approval_mode, &cwd).await {
        Ok(value) => Json(value),
        Err(err) => Json(json!({ "ok": false, "error": err.to_string() })),
    }
}

async fn jobs_handler(State(state): State<AppState>) -> Json<AppResponse> {
    let runtime = state.runtime.read().await;
    Json(runtime.app_status())
}

async fn mcp_startup_handler(State(state): State<AppState>) -> Json<Value> {
    let runtime = state.runtime.read().await;
    let summary = runtime.mcp_startup().await;
    Json(json!({
        "ok": true,
        "summary": summary
    }))
}

async fn app_handler(
    State(state): State<AppState>,
    Json(req): Json<AppRequest>,
) -> Json<AppResponse> {
    Json(process_app_request(&state, req, AppTransport::Http).await)
}

pub fn build_state(config_path: Option<PathBuf>, auth_token: Option<String>) -> Result<AppState> {
    let has_explicit_config_path = config_path.is_some();
    let store = ConfigStore::load(config_path)?;
    let config_path = has_explicit_config_path.then(|| store.path().to_path_buf());
    let config = store.config.clone();
    let exec_policy = store.exec_policy_engine();
    let registry = ModelRegistry::default();

    let state_db_path = config_path
        .as_ref()
        .and_then(|p| p.parent().map(|parent| parent.join("state.db")));
    let state_store = StateStore::open(state_db_path)?;

    let mut hooks = HookDispatcher::default();
    hooks.add_sink(Arc::new(StdoutHookSink));
    let hook_log_path = config_path
        .as_ref()
        .and_then(|p| p.parent().map(|parent| parent.join("events.jsonl")))
        .unwrap_or_else(|| PathBuf::from(".mimofan/events.jsonl"));
    hooks.add_sink(Arc::new(JsonlHookSink::new(hook_log_path)));

    if let Some(socket_path) = config
        .hook_sinks
        .as_ref()
        .and_then(|sinks| sinks.unix_socket_path.as_ref())
        .filter(|path| !path.as_os_str().is_empty())
    {
        hooks.add_sink(Arc::new(UnixSocketHookSink::new(socket_path.clone())));
    }

    let runtime = Runtime::new(
        config.clone(),
        registry.clone(),
        state_store,
        Arc::new(ToolRegistry::default()),
        Arc::new(McpManager::default()),
        exec_policy,
        hooks,
    );

    Ok(AppState {
        config_path,
        config: Arc::new(RwLock::new(config)),
        runtime: Arc::new(RwLock::new(runtime)),
        registry,
        auth_token,
    })
}

pub fn resolve_auth_token(options: &AppServerOptions) -> Result<Option<String>> {
    let configured = options.auth_token.as_ref().map(|token| token.trim());
    if let Some(token) = configured
        && token.is_empty()
    {
        bail!("app-server auth token cannot be empty");
    }
    let has_explicit_token = configured.is_some();

    if options.insecure_no_auth {
        if !options.listen.ip().is_loopback() {
            bail!("refusing unauthenticated app-server bind on non-loopback address");
        }
        eprintln!("warning: app-server HTTP auth disabled by --insecure-no-auth");
        return Ok(None);
    }

    if !has_explicit_token && !options.listen.ip().is_loopback() {
        bail!(
            "refusing non-loopback app-server bind without explicit auth token; pass --auth-token or set MIMOFAN_APP_SERVER_TOKEN"
        );
    }

    let token = configured
        .map(str::to_string)
        .unwrap_or_else(|| format!("cwapp_{}", Uuid::new_v4().simple()));
    for line in app_server_auth_status_lines(has_explicit_token) {
        eprintln!("{line}");
    }
    Ok(Some(token))
}

pub fn app_server_auth_status_lines(has_explicit_token: bool) -> Vec<&'static str> {
    if has_explicit_token {
        return vec!["app-server auth: bearer token required for HTTP routes."];
    }
    vec![
        "app-server auth: generated bearer token for this process (not printed).",
        "  Pass --auth-token or set MIMOFAN_APP_SERVER_TOKEN when another client needs to connect.",
    ]
}

pub fn cors_layer(extra_origins: &[String]) -> CorsLayer {
    let mut origins: Vec<HeaderValue> = DEFAULT_CORS_ORIGINS
        .iter()
        .filter_map(|origin| HeaderValue::from_str(origin).ok())
        .collect();
    for raw in extra_origins {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        match HeaderValue::from_str(trimmed) {
            Ok(value) if !origins.contains(&value) => origins.push(value),
            Ok(_) => {}
            Err(err) => {
                eprintln!("warning: ignoring invalid app-server CORS origin `{trimmed}`: {err}")
            }
        }
    }

    CorsLayer::new()
        .allow_origin(origins)
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE])
}

async fn require_app_server_token(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    let Some(expected) = state.auth_token.as_deref() else {
        return next.run(req).await;
    };
    let authorized = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|raw| raw.strip_prefix("Bearer "))
        .is_some_and(|token| token == expected);

    if authorized {
        next.run(req).await
    } else {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "error": {
                    "message": "app-server bearer token required",
                    "status": StatusCode::UNAUTHORIZED.as_u16(),
                }
            })),
        )
            .into_response()
    }
}

pub fn params_or_object(params: Value) -> Value {
    if params.is_null() { json!({}) } else { params }
}

fn parse_params<T: DeserializeOwned>(params: Value) -> std::result::Result<T, JsonRpcError> {
    serde_json::from_value(params).map_err(|err| JsonRpcError::invalid_params(err.to_string()))
}

pub fn jsonrpc_result(id: Option<Value>, result: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id.unwrap_or(Value::Null),
        "result": result
    })
}

pub fn jsonrpc_error(id: Option<Value>, err: JsonRpcError) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id.unwrap_or(Value::Null),
        "error": {
            "code": err.code,
            "message": err.message,
            "data": err.data
        }
    })
}

impl JsonRpcError {
    pub fn parse_error(message: impl Into<String>) -> Self {
        Self {
            code: -32700,
            message: message.into(),
            data: None,
        }
    }

    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self {
            code: -32600,
            message: message.into(),
            data: None,
        }
    }

    pub fn method_not_found(method: &str) -> Self {
        Self {
            code: -32601,
            message: format!("unsupported method: {method}"),
            data: None,
        }
    }

    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self {
            code: -32602,
            message: message.into(),
            data: None,
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            code: -32603,
            message: message.into(),
            data: None,
        }
    }
}

async fn handle_thread_request(
    state: &AppState,
    req: ThreadRequest,
) -> std::result::Result<ThreadResponse, JsonRpcError> {
    let mut runtime = state.runtime.write().await;
    runtime
        .handle_thread(req)
        .await
        .map_err(|err| JsonRpcError::internal(err.to_string()))
}

async fn handle_prompt_request(
    state: &AppState,
    req: PromptRequest,
) -> std::result::Result<PromptResponse, JsonRpcError> {
    let mut runtime = state.runtime.write().await;
    runtime
        .handle_prompt(req, &CliRuntimeOverrides::default())
        .await
        .map_err(|err| JsonRpcError::internal(err.to_string()))
}

pub async fn dispatch_stdio_request(
    state: &AppState,
    method: &str,
    params: Value,
) -> std::result::Result<StdioDispatchResult, JsonRpcError> {
    let outcome = match method {
        "healthz" | "app/healthz" => StdioDispatchResult {
            result: json!({
                "status": "ok",
                "service": "deepseek-app-server",
                "transport": "stdio"
            }),
            should_exit: false,
        },
        "capabilities" => StdioDispatchResult {
            result: json!({
                "transport": "stdio",
                "families": ["thread/*", "app/*", "prompt/*"],
                "methods": [
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
                    "shutdown"
                ]
            }),
            should_exit: false,
        },
        "thread/capabilities" => StdioDispatchResult {
            result: json!({
                "methods": [
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
                    "thread/message"
                ]
            }),
            should_exit: false,
        },
        "thread/request" => {
            let request: ThreadRequest = parse_params(params)?;
            let response = handle_thread_request(state, request).await?;
            StdioDispatchResult {
                result: serde_json::to_value(response)
                    .map_err(|err| JsonRpcError::internal(err.to_string()))?,
                should_exit: false,
            }
        }
        "thread/create" => {
            #[derive(Debug, Deserialize)]
            struct CreateParams {
                #[serde(default)]
                metadata: Value,
            }
            let parsed: CreateParams = parse_params(params_or_object(params))?;
            let response = handle_thread_request(
                state,
                ThreadRequest::Create {
                    metadata: parsed.metadata,
                },
            )
            .await?;
            StdioDispatchResult {
                result: serde_json::to_value(response)
                    .map_err(|err| JsonRpcError::internal(err.to_string()))?,
                should_exit: false,
            }
        }
        "thread/start" => {
            let request = ThreadRequest::Start(parse_params(params_or_object(params))?);
            let response = handle_thread_request(state, request).await?;
            StdioDispatchResult {
                result: serde_json::to_value(response)
                    .map_err(|err| JsonRpcError::internal(err.to_string()))?,
                should_exit: false,
            }
        }
        "thread/resume" => {
            let request = ThreadRequest::Resume(parse_params(params_or_object(params))?);
            let response = handle_thread_request(state, request).await?;
            StdioDispatchResult {
                result: serde_json::to_value(response)
                    .map_err(|err| JsonRpcError::internal(err.to_string()))?,
                should_exit: false,
            }
        }
        "thread/fork" => {
            let request = ThreadRequest::Fork(parse_params(params_or_object(params))?);
            let response = handle_thread_request(state, request).await?;
            StdioDispatchResult {
                result: serde_json::to_value(response)
                    .map_err(|err| JsonRpcError::internal(err.to_string()))?,
                should_exit: false,
            }
        }
        "thread/list" => {
            let request = ThreadRequest::List(parse_params(params_or_object(params))?);
            let response = handle_thread_request(state, request).await?;
            StdioDispatchResult {
                result: serde_json::to_value(response)
                    .map_err(|err| JsonRpcError::internal(err.to_string()))?,
                should_exit: false,
            }
        }
        "thread/read" => {
            let request = ThreadRequest::Read(parse_params(params_or_object(params))?);
            let response = handle_thread_request(state, request).await?;
            StdioDispatchResult {
                result: serde_json::to_value(response)
                    .map_err(|err| JsonRpcError::internal(err.to_string()))?,
                should_exit: false,
            }
        }
        "thread/set_name" | "thread/set-name" => {
            let request = ThreadRequest::SetName(parse_params(params_or_object(params))?);
            let response = handle_thread_request(state, request).await?;
            StdioDispatchResult {
                result: serde_json::to_value(response)
                    .map_err(|err| JsonRpcError::internal(err.to_string()))?,
                should_exit: false,
            }
        }
        "thread/goal/set" | "thread/goal_set" | "thread/goal-set" => {
            let request = ThreadRequest::GoalSet(parse_params::<ThreadGoalSetParams>(
                params_or_object(params),
            )?);
            let response = handle_thread_request(state, request).await?;
            StdioDispatchResult {
                result: serde_json::to_value(response)
                    .map_err(|err| JsonRpcError::internal(err.to_string()))?,
                should_exit: false,
            }
        }
        "thread/goal/get" | "thread/goal_get" | "thread/goal-get" => {
            let request = ThreadRequest::GoalGet(parse_params::<ThreadGoalGetParams>(
                params_or_object(params),
            )?);
            let response = handle_thread_request(state, request).await?;
            StdioDispatchResult {
                result: serde_json::to_value(response)
                    .map_err(|err| JsonRpcError::internal(err.to_string()))?,
                should_exit: false,
            }
        }
        "thread/goal/clear" | "thread/goal_clear" | "thread/goal-clear" => {
            let request = ThreadRequest::GoalClear(parse_params::<ThreadGoalClearParams>(
                params_or_object(params),
            )?);
            let response = handle_thread_request(state, request).await?;
            StdioDispatchResult {
                result: serde_json::to_value(response)
                    .map_err(|err| JsonRpcError::internal(err.to_string()))?,
                should_exit: false,
            }
        }
        "thread/archive" => {
            let parsed: ThreadIdParams = parse_params(params_or_object(params))?;
            let response = handle_thread_request(
                state,
                ThreadRequest::Archive {
                    thread_id: parsed.thread_id,
                },
            )
            .await?;
            StdioDispatchResult {
                result: serde_json::to_value(response)
                    .map_err(|err| JsonRpcError::internal(err.to_string()))?,
                should_exit: false,
            }
        }
        "thread/unarchive" => {
            let parsed: ThreadIdParams = parse_params(params_or_object(params))?;
            let response = handle_thread_request(
                state,
                ThreadRequest::Unarchive {
                    thread_id: parsed.thread_id,
                },
            )
            .await?;
            StdioDispatchResult {
                result: serde_json::to_value(response)
                    .map_err(|err| JsonRpcError::internal(err.to_string()))?,
                should_exit: false,
            }
        }
        "thread/message" => {
            let parsed: ThreadMessageParams = parse_params(params_or_object(params))?;
            let response = handle_thread_request(
                state,
                ThreadRequest::Message {
                    thread_id: parsed.thread_id,
                    input: parsed.input,
                },
            )
            .await?;
            StdioDispatchResult {
                result: serde_json::to_value(response)
                    .map_err(|err| JsonRpcError::internal(err.to_string()))?,
                should_exit: false,
            }
        }
        "app/capabilities" => {
            let response =
                process_app_request(state, AppRequest::Capabilities, AppTransport::Stdio).await;
            StdioDispatchResult {
                result: serde_json::to_value(response)
                    .map_err(|err| JsonRpcError::internal(err.to_string()))?,
                should_exit: false,
            }
        }
        "app/request" => {
            let request: AppRequest = parse_params(params)?;
            let response = process_app_request(state, request, AppTransport::Stdio).await;
            StdioDispatchResult {
                result: serde_json::to_value(response)
                    .map_err(|err| JsonRpcError::internal(err.to_string()))?,
                should_exit: false,
            }
        }
        "app/config/get" => {
            let parsed: ConfigGetParams = parse_params(params_or_object(params))?;
            let response = process_app_request(
                state,
                AppRequest::ConfigGet { key: parsed.key },
                AppTransport::Stdio,
            )
            .await;
            StdioDispatchResult {
                result: serde_json::to_value(response)
                    .map_err(|err| JsonRpcError::internal(err.to_string()))?,
                should_exit: false,
            }
        }
        "app/config/set" => {
            let parsed: ConfigSetParams = parse_params(params_or_object(params))?;
            let response = process_app_request(
                state,
                AppRequest::ConfigSet {
                    key: parsed.key,
                    value: parsed.value,
                },
                AppTransport::Stdio,
            )
            .await;
            StdioDispatchResult {
                result: serde_json::to_value(response)
                    .map_err(|err| JsonRpcError::internal(err.to_string()))?,
                should_exit: false,
            }
        }
        "app/config/unset" => {
            let parsed: ConfigGetParams = parse_params(params_or_object(params))?;
            let response = process_app_request(
                state,
                AppRequest::ConfigUnset { key: parsed.key },
                AppTransport::Stdio,
            )
            .await;
            StdioDispatchResult {
                result: serde_json::to_value(response)
                    .map_err(|err| JsonRpcError::internal(err.to_string()))?,
                should_exit: false,
            }
        }
        "app/config/list" => {
            let response =
                process_app_request(state, AppRequest::ConfigList, AppTransport::Stdio).await;
            StdioDispatchResult {
                result: serde_json::to_value(response)
                    .map_err(|err| JsonRpcError::internal(err.to_string()))?,
                should_exit: false,
            }
        }
        "app/models" => {
            let response =
                process_app_request(state, AppRequest::Models, AppTransport::Stdio).await;
            StdioDispatchResult {
                result: serde_json::to_value(response)
                    .map_err(|err| JsonRpcError::internal(err.to_string()))?,
                should_exit: false,
            }
        }
        "app/thread_loaded_list" | "app/thread-loaded-list" => {
            let response =
                process_app_request(state, AppRequest::ThreadLoadedList, AppTransport::Stdio).await;
            StdioDispatchResult {
                result: serde_json::to_value(response)
                    .map_err(|err| JsonRpcError::internal(err.to_string()))?,
                should_exit: false,
            }
        }
        "prompt/capabilities" => StdioDispatchResult {
            result: json!({
                "methods": ["prompt/request", "prompt/run"]
            }),
            should_exit: false,
        },
        "prompt/request" | "prompt/run" => {
            let request: PromptRequest = parse_params(params)?;
            let response = handle_prompt_request(state, request).await?;
            StdioDispatchResult {
                result: serde_json::to_value(response)
                    .map_err(|err| JsonRpcError::internal(err.to_string()))?,
                should_exit: false,
            }
        }
        "shutdown" => StdioDispatchResult {
            result: json!({"ok": true, "status": "stopped"}),
            should_exit: true,
        },
        _ => return Err(JsonRpcError::method_not_found(method)),
    };
    Ok(outcome)
}

pub async fn process_app_request(
    state: &AppState,
    req: AppRequest,
    transport: AppTransport,
) -> AppResponse {
    match req {
        AppRequest::Capabilities => AppResponse {
            ok: true,
            data: json!({
                "routes": ["/thread", "/app", "/prompt", "/tool", "/jobs", "/mcp/startup"],
                "config": ["get", "set", "unset", "list"],
                "events": ["response_start", "response_delta", "response_end", "tool_call_start", "tool_call_result", "mcp_startup_update", "mcp_startup_complete"],
                "transport": "stdio+http",
                "config_path": state.config_path.as_ref().map(|p| p.display().to_string()),
            }),
            events: Vec::new(),
        },
        AppRequest::ConfigGet { key } => {
            let cfg = state.config.read().await;
            let value = match transport {
                AppTransport::Http => cfg.get_display_value(&key),
                AppTransport::Stdio => cfg.get_value(&key),
            };
            AppResponse {
                ok: true,
                data: json!({ "key": key, "value": value }),
                events: Vec::new(),
            }
        }
        AppRequest::ConfigSet { key, value } => {
            let mut cfg = state.config.write().await;
            let result = cfg.set_value(&key, &value);
            let ok = result.is_ok();
            let message = result.err().map(|e| e.to_string());
            let snapshot = cfg.clone();
            drop(cfg);
            if let Err(e) = persist_config(state, snapshot).await {
                tracing::error!("Failed to persist config after set: {e}");
            }
            AppResponse {
                ok,
                data: json!({ "key": key, "value": value, "error": message }),
                events: Vec::new(),
            }
        }
        AppRequest::ConfigUnset { key } => {
            let mut cfg = state.config.write().await;
            let result = cfg.unset_value(&key);
            let ok = result.is_ok();
            let message = result.err().map(|e| e.to_string());
            let snapshot = cfg.clone();
            drop(cfg);
            if let Err(e) = persist_config(state, snapshot).await {
                tracing::error!("Failed to persist config after unset: {e}");
            }
            AppResponse {
                ok,
                data: json!({ "key": key, "error": message }),
                events: Vec::new(),
            }
        }
        AppRequest::ConfigList => {
            let cfg = state.config.read().await;
            AppResponse {
                ok: true,
                data: json!({ "values": cfg.list_values() }),
                events: Vec::new(),
            }
        }
        AppRequest::Models => AppResponse {
            ok: true,
            data: json!({ "models": state.registry.list() }),
            events: Vec::new(),
        },
        AppRequest::ThreadLoadedList => {
            let mut runtime = state.runtime.write().await;
            let response = runtime
                .handle_thread(mimofan_protocol::ThreadRequest::List(
                    mimofan_protocol::ThreadListParams {
                        include_archived: false,
                        limit: Some(50),
                    },
                ))
                .await;
            match response {
                Ok(thread_resp) => AppResponse {
                    ok: true,
                    data: json!({ "threads": thread_resp.threads }),
                    events: thread_resp.events,
                },
                Err(err) => AppResponse {
                    ok: false,
                    data: json!({ "error": err.to_string() }),
                    events: Vec::new(),
                },
            }
        }
    }
}

async fn persist_config(state: &AppState, config: mimofan_config::ConfigToml) -> Result<()> {
    if state.config_path.is_none() {
        return Ok(());
    }
    let mut store = ConfigStore::load(state.config_path.clone())?;
    store.config = config;
    store.save()
}
