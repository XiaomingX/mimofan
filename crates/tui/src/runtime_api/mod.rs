//! Runtime HTTP/SSE API for local mimofan automation.

mod automation_routes;
mod fleet_routes;
mod session_routes;
mod skills_routes;
mod sse;
mod thread_routes;
mod types;
mod workspace_routes;

use std::net::{SocketAddr, UdpSocket};
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow, bail};
use axum::extract::{Request, State};
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::Html;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tower_http::cors::{Any, CorsLayer};

use crate::automation_manager::{AutomationManager, AutomationSchedulerConfig, spawn_scheduler};
use crate::config::Config;
use crate::mcp::McpPool;
use crate::runtime_threads::RuntimeThreadManager;
use crate::runtime_threads::requests::RuntimeThreadManagerConfig;
use crate::session_manager::default_sessions_dir;
use crate::skill_state::SkillStateStore;
use crate::task_manager::{TaskManager, TaskManagerConfig};

pub(crate) use types::runtime_api_sub_agent_manager;

#[derive(Debug, Clone)]
pub(crate) struct RuntimeApiOptions {
    pub host: String,
    pub port: u16,
    pub workers: usize,
    /// Additional CORS origins to allow on top of the built-in defaults.
    pub cors_origins: Vec<String>,
    /// Optional bearer token required for `/v1/*` routes.
    pub auth_token: Option<String>,
    /// Allow `/v1/*` routes without auth when no token is configured.
    pub insecure_no_auth: bool,
    /// Enables the built-in mobile control page at `/mobile`.
    pub mobile: bool,
    /// Show a QR code for the mobile URL in the terminal.
    pub show_qr: bool,
}

impl Default for RuntimeApiOptions {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 7878,
            workers: 2,
            cors_origins: Vec::new(),
            auth_token: None,
            insecure_no_auth: false,
            mobile: false,
            show_qr: false,
        }
    }
}

#[derive(Clone)]
pub(crate) struct RuntimeApiState {
    pub(crate) config: Config,
    pub(crate) workspace: PathBuf,
    pub(crate) task_manager: crate::task_manager::SharedTaskManager,
    pub(crate) runtime_threads: crate::runtime_threads::SharedRuntimeThreadManager,
    pub(crate) cors_origins: Vec<String>,
    pub(crate) sessions_dir: PathBuf,
    pub(crate) mcp_config_path: PathBuf,
    pub(crate) automations: crate::automation_manager::SharedAutomationManager,
    pub(crate) sub_agent_manager: crate::tools::subagent::SharedSubAgentManager,
    pub(crate) runtime_token: Option<String>,
    pub(crate) skill_state: Arc<Mutex<SkillStateStore>>,
    pub(crate) auth_required: bool,
    pub(crate) bind_host: String,
    pub(crate) bind_port: u16,
    pub(crate) mobile_enabled: bool,
    /// Shared McpPool reused across HTTP API calls so each call does not
    /// spawn a duplicate set of MCP server processes.
    pub(crate) mcp_pool: Arc<Mutex<Option<McpPool>>>,
}

// ── Auth helpers ────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedRuntimeAuth {
    token: Option<String>,
    generated: bool,
}

fn resolve_runtime_auth(
    cli_token: Option<String>,
    env_token: Option<String>,
    insecure_no_auth: bool,
) -> ResolvedRuntimeAuth {
    if let Some(token) = first_nonblank_token(cli_token).or_else(|| first_nonblank_token(env_token))
    {
        return ResolvedRuntimeAuth {
            token: Some(token),
            generated: false,
        };
    }
    if insecure_no_auth {
        return ResolvedRuntimeAuth {
            token: None,
            generated: false,
        };
    }
    ResolvedRuntimeAuth {
        token: Some(generate_runtime_token()),
        generated: true,
    }
}

fn runtime_auth_status_lines(auth: &ResolvedRuntimeAuth) -> Vec<String> {
    if auth.generated {
        return vec![
            "Runtime API auth: generated bearer token for this process (not printed).".to_string(),
            "  Set MIMOFAN_RUNTIME_TOKEN (or MIMOFAN_RUNTIME_TOKEN as an alias) or pass --auth-token when another client needs to connect.".to_string(),
        ];
    }
    if auth.token.is_some() {
        return vec!["Runtime API auth: bearer token required for /v1/* routes.".to_string()];
    }
    vec!["Runtime API auth: disabled by explicit insecure mode.".to_string()]
}

fn first_nonblank_token(token: Option<String>) -> Option<String> {
    token
        .map(|token| token.trim().to_string())
        .filter(|token| !token.is_empty())
}

fn generate_runtime_token() -> String {
    format!(
        "cwrt_{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

// ── Auth middleware ──────────────────────────────────────────────────

async fn require_runtime_token(
    State(state): State<RuntimeApiState>,
    req: Request,
    next: Next,
) -> Response {
    let Some(expected) = state.runtime_token.as_deref() else {
        return next.run(req).await;
    };
    let authorized = request_has_runtime_token(&req, expected);

    if authorized {
        next.run(req).await
    } else {
        runtime_token_required_response()
    }
}

fn request_has_runtime_token(req: &Request, expected: &str) -> bool {
    req.headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|raw| raw.strip_prefix("Bearer "))
        .is_some_and(|token| token == expected)
        || req
            .headers()
            .get("x-mimofan-runtime-token")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|token| token == expected)
        || req
            .headers()
            .get("x-deepseek-runtime-token")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|token| token == expected)
        || token_from_cookie_header(
            req.headers()
                .get(header::COOKIE)
                .and_then(|value| value.to_str().ok()),
        )
        .is_some_and(|token| token == expected)
}

fn runtime_token_required_response() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({
            "error": {
                "message": "runtime API bearer token required",
                "status": StatusCode::UNAUTHORIZED.as_u16(),
            }
        })),
    )
        .into_response()
}

const RUNTIME_TOKEN_COOKIE: &str = "mimofan_runtime_token";

fn token_from_cookie_header(cookie: Option<&str>) -> Option<String> {
    cookie.and_then(|cookie| {
        cookie.split(';').find_map(|pair| {
            let pair = pair.trim();
            let (key, value) = pair.split_once('=')?;
            (key == RUNTIME_TOKEN_COOKIE)
                .then(|| percent_decode_query_component(value.trim()))
                .flatten()
        })
    })
}

fn percent_decode_query_component(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' => {
                let hi = *bytes.get(index + 1)?;
                let lo = *bytes.get(index + 2)?;
                let hi = (hi as char).to_digit(16)? as u8;
                let lo = (lo as char).to_digit(16)? as u8;
                decoded.push((hi << 4) | lo);
                index += 3;
            }
            b'+' => {
                decoded.push(b' ');
                index += 1;
            }
            byte => {
                decoded.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(decoded).ok()
}

// ── Mobile page ─────────────────────────────────────────────────────

const MOBILE_HTML: &str = include_str!("../runtime_mobile.html");

async fn mobile_page(State(state): State<RuntimeApiState>, req: Request) -> Response {
    if !state.mobile_enabled {
        return (
            StatusCode::NOT_FOUND,
            "mobile control is disabled; start with `mimofan serve --mobile`",
        )
            .into_response();
    }
    let _ = req;
    Html(MOBILE_HTML).into_response()
}

fn print_mobile_urls(addr: SocketAddr, auth_enabled: bool, generated_auth: bool, show_qr: bool) {
    println!("Mobile control page enabled.");

    let port = addr.port();
    let qr_url = if addr.ip().is_unspecified() {
        println!("  Local: http://127.0.0.1:{port}/mobile");
        if let Some(ip) = detect_lan_ip() {
            let lan_url = format!("http://{ip}:{port}/mobile");
            println!("  LAN:   {lan_url}");
            lan_url
        } else {
            println!("  LAN:   bind is 0.0.0.0; open http://<this-machine-ip>:{port}/mobile");
            format!("http://127.0.0.1:{port}/mobile")
        }
    } else {
        let url = format!("http://{addr}/mobile");
        println!("  URL:   {url}");
        url
    };
    if auth_enabled {
        if generated_auth {
            println!(
                "  Auth uses an unprinted generated token; restart with MIMOFAN_RUNTIME_TOKEN or --auth-token to sign in from another client."
            );
        } else {
            println!("  Enter the configured runtime token in the page connection field.");
        }
    }
    println!("Mobile security: use only on a trusted LAN/VPN; this server does not provide TLS.");

    if show_qr {
        match qrcode::QrCode::new(qr_url.as_bytes()) {
            Ok(qr) => {
                let qr_str = qr.render::<qrcode::render::unicode::Dense1x2>().build();
                println!("\n{qr_str}");
            }
            Err(e) => {
                eprintln!("Warning: could not generate QR code: {e}");
            }
        }
    }
}

fn detect_lan_ip() -> Option<String> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    // UDP connect only selects the outbound interface locally; no packet is sent.
    socket.connect("10.255.255.255:1").ok()?;
    let addr = socket.local_addr().ok()?;
    Some(addr.ip().to_string())
}

// ── CORS layer ──────────────────────────────────────────────────────

/// Built-in dev origins always allowed by the runtime API.
const DEFAULT_CORS_ORIGINS: &[&str] = &[
    "http://localhost:3000",
    "http://127.0.0.1:3000",
    "http://localhost:1420",
    "http://127.0.0.1:1420",
    "tauri://localhost",
];

fn cors_layer(extra_origins: &[String]) -> CorsLayer {
    let mut origins: Vec<HeaderValue> = DEFAULT_CORS_ORIGINS
        .iter()
        .filter_map(|o| HeaderValue::from_str(o).ok())
        .collect();
    for raw in extra_origins {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        match HeaderValue::from_str(trimmed) {
            Ok(value) if !origins.contains(&value) => origins.push(value),
            Ok(_) => {}
            Err(err) => tracing::warn!(
                "Ignoring invalid CORS origin '{trimmed}': {err}; expected scheme://host[:port]"
            ),
        }
    }
    CorsLayer::new()
        .allow_origin(origins)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers(Any)
}

// ── Health endpoint ─────────────────────────────────────────────────

async fn health() -> Json<types::HealthResponse> {
    Json(types::HealthResponse {
        status: "ok",
        service: "mimofan-runtime-api",
        mode: "local",
    })
}

// ── Router ──────────────────────────────────────────────────────────

pub(crate) fn build_router(state: RuntimeApiState) -> Router {
    let api_routes = Router::new()
        .route(
            "/v1/sessions",
            get(session_routes::list_sessions)
                .post(session_routes::create_session_from_thread)
                .put(session_routes::save_current_session),
        )
        .route(
            "/v1/sessions/{id}",
            get(session_routes::get_session).delete(session_routes::delete_session),
        )
        .route(
            "/v1/sessions/{id}/resume-thread",
            post(session_routes::resume_session_thread),
        )
        .route(
            "/v1/workspace/status",
            get(workspace_routes::workspace_status),
        )
        .route("/v1/agent-runs", get(fleet_routes::list_agent_runs))
        .route("/v1/agent-runs/{run_id}", get(fleet_routes::get_agent_run))
        .route("/v1/fleet/runs", get(fleet_routes::list_fleet_runs))
        .route("/v1/fleet/runs/{run_id}", get(fleet_routes::get_fleet_run))
        .route(
            "/v1/fleet/runs/{run_id}/workers",
            get(fleet_routes::list_fleet_run_workers),
        )
        .route(
            "/v1/fleet/runs/{run_id}/stop",
            post(fleet_routes::stop_fleet_run),
        )
        .route(
            "/v1/fleet/workers/{worker_id}",
            get(fleet_routes::get_fleet_worker),
        )
        .route(
            "/v1/fleet/workers/{worker_id}/interrupt",
            post(fleet_routes::interrupt_fleet_worker),
        )
        .route(
            "/v1/fleet/workers/{worker_id}/restart",
            post(fleet_routes::restart_fleet_worker),
        )
        .route("/v1/stream", post(sse::stream_turn))
        .route(
            "/v1/threads",
            get(thread_routes::list_threads).post(thread_routes::create_thread),
        )
        .route(
            "/v1/threads/summary",
            get(thread_routes::list_threads_summary),
        )
        .route(
            "/v1/threads/{id}",
            get(thread_routes::get_thread).patch(thread_routes::update_thread),
        )
        .route(
            "/v1/threads/{id}/resume",
            post(thread_routes::resume_thread),
        )
        .route("/v1/threads/{id}/fork", post(thread_routes::fork_thread))
        .route(
            "/v1/threads/{id}/undo",
            post(thread_routes::undo_thread_turn),
        )
        .route(
            "/v1/threads/{id}/patch-undo",
            post(thread_routes::patch_undo_thread_turn),
        )
        .route(
            "/v1/threads/{id}/retry",
            post(thread_routes::retry_thread_turn),
        )
        .route(
            "/v1/threads/{id}/turns",
            post(thread_routes::start_thread_turn),
        )
        .route(
            "/v1/threads/{id}/turns/{turn_id}/steer",
            post(thread_routes::steer_thread_turn),
        )
        .route(
            "/v1/threads/{id}/turns/{turn_id}/interrupt",
            post(thread_routes::interrupt_thread_turn),
        )
        .route(
            "/v1/threads/{id}/turns/{turn_id}/tool-calls/{call_id}/result",
            post(thread_routes::deliver_dynamic_tool_result),
        )
        .route(
            "/v1/threads/{id}/compact",
            post(thread_routes::compact_thread),
        )
        .route("/v1/threads/{id}/events", get(sse::stream_thread_events))
        .route(
            "/v1/approvals/{approval_id}",
            post(thread_routes::decide_approval),
        )
        .route(
            "/v1/user-input/{thread_id}/{input_id}",
            post(thread_routes::submit_user_input),
        )
        .route(
            "/v1/tasks",
            get(thread_routes::list_tasks).post(thread_routes::create_task),
        )
        .route("/v1/tasks/{id}", get(thread_routes::get_task))
        .route("/v1/tasks/{id}/cancel", post(thread_routes::cancel_task))
        .route("/v1/skills", get(skills_routes::list_skills))
        .route("/v1/skills/{name}", post(skills_routes::set_skill_enabled))
        .route("/v1/apps/mcp/servers", get(skills_routes::list_mcp_servers))
        .route("/v1/apps/mcp/tools", get(skills_routes::list_mcp_tools))
        .route(
            "/v1/automations",
            get(automation_routes::list_automations).post(automation_routes::create_automation),
        )
        .route(
            "/v1/automations/{id}",
            get(automation_routes::get_automation)
                .patch(automation_routes::update_automation)
                .delete(automation_routes::delete_automation),
        )
        .route(
            "/v1/automations/{id}/run",
            post(automation_routes::run_automation),
        )
        .route(
            "/v1/automations/{id}/pause",
            post(automation_routes::pause_automation),
        )
        .route(
            "/v1/automations/{id}/resume",
            post(automation_routes::resume_automation),
        )
        .route(
            "/v1/automations/{id}/runs",
            get(automation_routes::list_automation_runs),
        )
        .route("/v1/usage", get(thread_routes::get_usage))
        .route("/v1/snapshots", get(thread_routes::list_snapshots))
        .route(
            "/v1/snapshots/{id}/restore",
            post(thread_routes::restore_snapshot),
        )
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            require_runtime_token,
        ));

    Router::new()
        .route("/health", get(health))
        .route("/mobile", get(mobile_page))
        .route("/mobile/", get(mobile_page))
        .route("/v1/runtime/info", get(skills_routes::runtime_info))
        .merge(api_routes)
        .layer(cors_layer(&state.cors_origins))
        .with_state(state)
}

// ── Server entry point ──────────────────────────────────────────────

/// Start the runtime API server.
pub(crate) async fn run_http_server(
    config: Config,
    workspace: PathBuf,
    options: RuntimeApiOptions,
    runtime: Option<std::sync::Arc<tokio::sync::RwLock<mimofan_core::Runtime>>>,
) -> Result<()> {
    if options.port == 0 {
        bail!("Port must be > 0");
    }

    let task_cfg = TaskManagerConfig::from_runtime(
        &config,
        workspace.clone(),
        config.default_text_model.clone(),
        Some(options.workers),
    );
    let runtime_threads = Arc::new(RuntimeThreadManager::open(
        config.clone(),
        workspace.clone(),
        RuntimeThreadManagerConfig::from_task_data_dir(task_cfg.data_dir.clone()),
        runtime,
    )?);
    let task_manager =
        TaskManager::start_with_runtime_manager(task_cfg, config.clone(), runtime_threads.clone())
            .await?;
    let automations = Arc::new(Mutex::new(AutomationManager::default_location()?));
    runtime_threads.attach_automation_manager(automations.clone());
    let scheduler_cancel = CancellationToken::new();
    let scheduler_handle = spawn_scheduler(
        automations.clone(),
        task_manager.clone(),
        scheduler_cancel.clone(),
        AutomationSchedulerConfig::default(),
    );

    let sessions_dir = default_sessions_dir().unwrap_or_else(|_| {
        dirs::home_dir()
            .map(|h| h.join(".mimofan").join("sessions"))
            .unwrap_or_else(|| PathBuf::from(".mimofan").join("sessions"))
    });
    let runtime_token_env = std::env::var("MIMOFAN_RUNTIME_TOKEN")
        .ok()
        .or_else(|| std::env::var("MIMOFAN_RUNTIME_TOKEN").ok());
    let resolved_auth = resolve_runtime_auth(
        options.auth_token.clone(),
        runtime_token_env,
        options.insecure_no_auth,
    );
    let runtime_token = resolved_auth.token.clone();
    let auth_enabled = runtime_token.is_some();
    let skill_state = SkillStateStore::load_default().unwrap_or_else(|err| {
        tracing::warn!(
            "Failed to load skills_state.toml ({}); treating all skills as enabled",
            err
        );
        SkillStateStore::default()
    });
    let sub_agent_manager = runtime_api_sub_agent_manager(&workspace, options.workers);
    let state = RuntimeApiState {
        config: config.clone(),
        workspace,
        task_manager,
        runtime_threads,
        cors_origins: options.cors_origins.clone(),
        sessions_dir,
        mcp_config_path: config.mcp_config_path(),
        automations,
        sub_agent_manager,
        runtime_token: runtime_token.clone(),
        skill_state: Arc::new(Mutex::new(skill_state)),
        auth_required: auth_enabled,
        bind_host: options.host.clone(),
        bind_port: options.port,
        mobile_enabled: options.mobile,
        mcp_pool: Arc::new(Mutex::new(None)),
    };
    let app = build_router(state);

    let addr: SocketAddr = format!("{}:{}", options.host, options.port)
        .parse()
        .with_context(|| format!("Invalid bind address '{}:{}'", options.host, options.port))?;
    let listener = TcpListener::bind(addr)
        .await
        .with_context(|| format!("Failed to bind {addr}"))?;

    println!("Runtime API listening on http://{addr}");
    for line in runtime_auth_status_lines(&resolved_auth) {
        println!("{line}");
    }
    if options.mobile {
        print_mobile_urls(addr, auth_enabled, resolved_auth.generated, options.show_qr);
    }
    let is_loopback = options.host == "127.0.0.1" || options.host == "::1";
    if is_loopback {
        println!("Security: this server is local-first. Do not expose it to untrusted networks.");
    } else {
        println!(
            "Security: bound to {host}; reachable from any peer that can route to this address.",
            host = options.host
        );
        if !auth_enabled {
            println!(
                "  WARNING: auth is disabled. Anyone on the network can call /v1/* without authentication."
            );
        }
        println!(
            "  /v1/runtime/info reports bind_host={host:?}, port={port}, auth_required={auth}.",
            host = options.host,
            port = options.port,
            auth = auth_enabled,
        );
    }
    let serve_result = axum::serve(listener, app)
        .await
        .map_err(|e| anyhow!("Runtime API server error: {e}"));
    scheduler_cancel.cancel();
    scheduler_handle.abort();
    serve_result
}
