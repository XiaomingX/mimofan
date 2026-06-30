mod job;
mod thread;

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use mimofan_agent::ModelRegistry;
use mimofan_config::{CliRuntimeOverrides, ConfigToml, DEFAULT_PROVIDER_ID, ProviderKind};
use mimofan_execpolicy::{
    AskForApproval, ExecApprovalRequirement, ExecPolicyContext, ExecPolicyDecision,
    ExecPolicyEngine,
};
use mimofan_hooks::{HookDispatcher, HookEvent};
use mimofan_mcp::{
    McpManager, McpStartupCompleteEvent, McpStartupStatus as McpManagerStartupStatus,
};
use mimofan_protocol::{
    AppResponse, EventFrame, ExecApprovalRequestEvent, PromptRequest, PromptResponse,
    ResponseChannel, ReviewDecision, ThreadRequest, ThreadResponse, ToolPayload,
    UserInputRequestEvent,
};
use mimofan_state::StateStore;
use mimofan_tools::{ToolCall, ToolRegistry};
use serde_json::{Value, json};
use uuid::Uuid;

// Re-export all public types for backward compatibility.
pub use job::*;
pub use thread::*;

/// Top-level runtime combining config, model registry, threads, tools, MCP, and hooks.
pub struct Runtime {
    /// Resolved application configuration.
    pub config: ConfigToml,
    /// Registry of available model providers.
    pub model_registry: ModelRegistry,
    /// Manages conversation thread lifecycle.
    pub thread_manager: ThreadManager,
    /// Registry of callable tools.
    pub tool_registry: Arc<ToolRegistry>,
    /// Manager for MCP server connections.
    pub mcp_manager: Arc<McpManager>,
    /// Engine for evaluating execution policy decisions.
    pub exec_policy: ExecPolicyEngine,
    /// Dispatcher for lifecycle hooks.
    pub hooks: HookDispatcher,
    /// Manager for background job lifecycle.
    pub jobs: JobManager,
}

impl Runtime {
    /// Constructs a new `Runtime`, loading existing jobs from the state store.
    pub fn new(
        config: ConfigToml,
        model_registry: ModelRegistry,
        state: StateStore,
        tool_registry: Arc<ToolRegistry>,
        mcp_manager: Arc<McpManager>,
        exec_policy: ExecPolicyEngine,
        hooks: HookDispatcher,
    ) -> Self {
        let mut jobs = JobManager::default();
        if let Err(e) = jobs.load_from_store(&state) {
            tracing::warn!("Failed to load job store, starting with empty job list: {e}");
        }
        Self {
            config,
            model_registry,
            thread_manager: ThreadManager::new(state),
            tool_registry,
            mcp_manager,
            exec_policy,
            hooks,
            jobs,
        }
    }

    fn persisted_thread_data(&self, thread_id: &str) -> Result<Value> {
        let history = self
            .thread_manager
            .state_store()
            .list_messages(thread_id, Some(500))
            .context("Failed to list messages for thread")?
            .into_iter()
            .map(|message| {
                json!({
                    "id": message.id,
                    "role": message.role,
                    "content": message.content,
                    "item": message.item,
                    "created_at": message.created_at
                })
            })
            .collect::<Vec<_>>();

        let checkpoint = self
            .thread_manager
            .state_store()
            .load_checkpoint(thread_id, None)
            .context("Failed to load checkpoint for thread")?
            .map(|record| {
                json!({
                    "checkpoint_id": record.checkpoint_id,
                    "state": record.state,
                    "created_at": record.created_at
                })
            });

        let goal = self
            .thread_manager
            .state_store()
            .get_thread_goal(thread_id)
            .context("Failed to get thread goal")?
            .map(to_protocol_goal);

        Ok(json!({
            "history": history,
            "checkpoint": checkpoint,
            "goal": goal
        }))
    }

    fn persist_latest_checkpoint(&self, thread_id: &str, reason: &str, state: Value) -> Result<()> {
        self.thread_manager
            .state_store()
            .save_checkpoint(
                thread_id,
                "latest",
                &json!({
                    "reason": reason,
                    "saved_at": chrono::Utc::now().timestamp(),
                    "state": state
                }),
            )
            .context("Failed to save checkpoint for thread")
    }

    /// Dispatches a thread request (create, start, resume, fork, list, read, etc.).
    pub async fn handle_thread(&mut self, req: ThreadRequest) -> Result<ThreadResponse> {
        match req {
            ThreadRequest::Create { .. } => {
                let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
                let new = self.thread_manager.spawn_thread_with_history(
                    DEFAULT_PROVIDER_ID.to_string(),
                    cwd,
                    InitialHistory::New,
                    false,
                )?;
                let mut response = thread_response_from_new("created", new);
                response.data = self.persisted_thread_data(&response.thread_id)?;
                Ok(response)
            }
            ThreadRequest::Start(params) => {
                let cwd = params.cwd.clone().unwrap_or_else(|| {
                    std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
                });
                let new = self.thread_manager.spawn_thread_with_history(
                    params
                        .model_provider
                        .clone()
                        .unwrap_or_else(|| DEFAULT_PROVIDER_ID.to_string()),
                    cwd,
                    InitialHistory::New,
                    params.persist_extended_history,
                )?;
                let mut response = thread_response_from_new("started", new);
                response.data = self.persisted_thread_data(&response.thread_id)?;
                Ok(response)
            }
            ThreadRequest::Resume(params) => {
                let fallback_cwd =
                    std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
                if let Some(new) = self.thread_manager.resume_thread_with_history(
                    &params,
                    &fallback_cwd,
                    DEFAULT_PROVIDER_ID.to_string(),
                )? {
                    let mut response = thread_response_from_new("resumed", new);
                    response.data = self.persisted_thread_data(&response.thread_id)?;
                    Ok(response)
                } else {
                    Ok(ThreadResponse {
                        thread_id: params.thread_id,
                        status: "missing".to_string(),
                        thread: None,
                        threads: Vec::new(),
                        goal: None,
                        model: None,
                        model_provider: None,
                        cwd: None,
                        approval_policy: params.approval_policy,
                        sandbox: params.sandbox,
                        events: Vec::new(),
                        data: json!({"error":"thread not found"}),
                    })
                }
            }
            ThreadRequest::Fork(params) => {
                let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
                if let Some(new) = self.thread_manager.fork_thread(&params, &cwd)? {
                    let mut response = thread_response_from_new("forked", new);
                    response.data = self.persisted_thread_data(&response.thread_id)?;
                    Ok(response)
                } else {
                    Ok(ThreadResponse {
                        thread_id: params.thread_id,
                        status: "missing".to_string(),
                        thread: None,
                        threads: Vec::new(),
                        goal: None,
                        model: None,
                        model_provider: None,
                        cwd: None,
                        approval_policy: params.approval_policy,
                        sandbox: params.sandbox,
                        events: Vec::new(),
                        data: json!({"error":"thread not found"}),
                    })
                }
            }
            ThreadRequest::List(params) => Ok(ThreadResponse {
                thread_id: "list".to_string(),
                status: "ok".to_string(),
                thread: None,
                threads: self.thread_manager.list_threads(&params)?,
                goal: None,
                model: None,
                model_provider: None,
                cwd: None,
                approval_policy: None,
                sandbox: None,
                events: Vec::new(),
                data: json!({}),
            }),
            ThreadRequest::Read(params) => {
                let id = params.thread_id.clone();
                let data = self.persisted_thread_data(&id)?;
                Ok(ThreadResponse {
                    thread_id: id,
                    status: "ok".to_string(),
                    thread: self.thread_manager.read_thread(&params)?,
                    threads: Vec::new(),
                    goal: self.thread_manager.get_thread_goal(&ThreadGoalGetParams {
                        thread_id: params.thread_id,
                    })?,
                    model: None,
                    model_provider: None,
                    cwd: None,
                    approval_policy: None,
                    sandbox: None,
                    events: Vec::new(),
                    data,
                })
            }
            ThreadRequest::SetName(params) => Ok(ThreadResponse {
                thread_id: params.thread_id.clone(),
                status: "ok".to_string(),
                thread: self.thread_manager.set_thread_name(&params)?,
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
            ThreadRequest::GoalSet(params) => {
                let thread_id = params.thread_id.clone();
                if let Some(goal) = self.thread_manager.set_thread_goal(&params)? {
                    Ok(ThreadResponse {
                        thread_id,
                        status: "ok".to_string(),
                        thread: None,
                        threads: Vec::new(),
                        goal: Some(goal.clone()),
                        model: None,
                        model_provider: None,
                        cwd: None,
                        approval_policy: None,
                        sandbox: None,
                        events: vec![EventFrame::ThreadGoalUpdated { goal: goal.clone() }],
                        data: json!({ "goal": goal }),
                    })
                } else {
                    Ok(ThreadResponse {
                        thread_id,
                        status: "missing".to_string(),
                        thread: None,
                        threads: Vec::new(),
                        goal: None,
                        model: None,
                        model_provider: None,
                        cwd: None,
                        approval_policy: None,
                        sandbox: None,
                        events: Vec::new(),
                        data: json!({"error":"thread not found"}),
                    })
                }
            }
            ThreadRequest::GoalGet(params) => {
                let goal = self.thread_manager.get_thread_goal(&params)?;
                Ok(ThreadResponse {
                    thread_id: params.thread_id,
                    status: "ok".to_string(),
                    thread: None,
                    threads: Vec::new(),
                    goal: goal.clone(),
                    model: None,
                    model_provider: None,
                    cwd: None,
                    approval_policy: None,
                    sandbox: None,
                    events: Vec::new(),
                    data: json!({ "goal": goal }),
                })
            }
            ThreadRequest::GoalClear(params) => {
                let thread_id = params.thread_id.clone();
                let cleared = self.thread_manager.clear_thread_goal(&params)?;
                Ok(ThreadResponse {
                    thread_id: thread_id.clone(),
                    status: if cleared { "cleared" } else { "empty" }.to_string(),
                    thread: None,
                    threads: Vec::new(),
                    goal: None,
                    model: None,
                    model_provider: None,
                    cwd: None,
                    approval_policy: None,
                    sandbox: None,
                    events: if cleared {
                        vec![EventFrame::ThreadGoalCleared { thread_id }]
                    } else {
                        Vec::new()
                    },
                    data: json!({ "cleared": cleared }),
                })
            }
            ThreadRequest::GoalRecordProgress(params) => {
                let thread_id = params.thread_id.clone();
                if let Some(goal) = self.thread_manager.record_thread_goal_progress(&params)? {
                    Ok(ThreadResponse {
                        thread_id,
                        status: "ok".to_string(),
                        thread: None,
                        threads: Vec::new(),
                        goal: Some(goal.clone()),
                        model: None,
                        model_provider: None,
                        cwd: None,
                        approval_policy: None,
                        sandbox: None,
                        events: vec![EventFrame::ThreadGoalUpdated { goal: goal.clone() }],
                        data: json!({ "goal": goal }),
                    })
                } else {
                    Ok(ThreadResponse {
                        thread_id,
                        status: "missing".to_string(),
                        thread: None,
                        threads: Vec::new(),
                        goal: None,
                        model: None,
                        model_provider: None,
                        cwd: None,
                        approval_policy: None,
                        sandbox: None,
                        events: Vec::new(),
                        data: json!({"error":"thread or goal not found"}),
                    })
                }
            }
            ThreadRequest::Archive { thread_id } => {
                self.thread_manager.archive_thread(&thread_id)?;
                Ok(ThreadResponse {
                    thread_id,
                    status: "archived".to_string(),
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
                })
            }
            ThreadRequest::Unarchive { thread_id } => {
                self.thread_manager.unarchive_thread(&thread_id)?;
                Ok(ThreadResponse {
                    thread_id,
                    status: "unarchived".to_string(),
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
                })
            }
            ThreadRequest::Message { thread_id, input } => {
                self.thread_manager.touch_message(&thread_id, &input)?;
                let response_id = format!("{thread_id}:{}", input.len());
                self.hooks
                    .emit(HookEvent::ResponseStart {
                        response_id: response_id.clone(),
                    })
                    .await;
                self.hooks
                    .emit(HookEvent::ResponseEnd {
                        response_id: response_id.clone(),
                    })
                    .await;

                Ok(ThreadResponse {
                    thread_id,
                    status: "accepted".to_string(),
                    thread: None,
                    threads: Vec::new(),
                    goal: None,
                    model: None,
                    model_provider: None,
                    cwd: None,
                    approval_policy: None,
                    sandbox: None,
                    events: vec![
                        EventFrame::ResponseStart {
                            response_id: response_id.clone(),
                        },
                        EventFrame::ResponseDelta {
                            response_id: response_id.clone(),
                            delta: "queued".to_string(),
                            channel: ResponseChannel::Text,
                        },
                        EventFrame::ResponseEnd { response_id },
                    ],
                    data: json!({}),
                })
            }
        }
    }

    /// Resolves the model for a prompt, records the message, and returns the response.
    pub async fn handle_prompt(
        &mut self,
        req: PromptRequest,
        cli_overrides: &CliRuntimeOverrides,
    ) -> Result<PromptResponse> {
        let resolved = self.config.resolve_runtime_options(cli_overrides);
        let requested_model = req.model.clone().unwrap_or_else(|| resolved.model.clone());
        let selection = self
            .model_registry
            .resolve(Some(&requested_model), Some(resolved.provider));
        let resolved_model = selection.resolved.id.clone();
        let response_id = format!("resp-{}", Uuid::new_v4());

        self.hooks
            .emit(HookEvent::ResponseStart {
                response_id: response_id.clone(),
            })
            .await;
        self.hooks
            .emit(HookEvent::ResponseDelta {
                response_id: response_id.clone(),
                delta: "model-selected".to_string(),
            })
            .await;
        self.hooks
            .emit(HookEvent::ResponseEnd {
                response_id: response_id.clone(),
            })
            .await;

        let payload = json!({
            "provider": resolved.provider.as_str(),
            "model": resolved_model.clone(),
            "prompt": req.prompt,
            "telemetry": resolved.telemetry,
            "base_url": resolved.base_url,
            "has_api_key": resolved.api_key.as_ref().is_some_and(|k| !k.trim().is_empty()),
            "approval_policy": resolved.approval_policy,
            "sandbox_mode": resolved.sandbox_mode
        });
        if let Some(thread_id) = req.thread_id.as_ref() {
            self.thread_manager.touch_message(thread_id, &req.prompt)?;
            let assistant_message_id = self.thread_manager.state_store().append_message(
                thread_id,
                "assistant",
                &payload.to_string(),
                Some(payload.clone()),
            )?;
            self.persist_latest_checkpoint(
                thread_id,
                "prompt_response",
                json!({
                    "response_id": response_id.clone(),
                    "model": resolved_model.clone(),
                    "provider": resolved.provider.as_str(),
                    "assistant_message_id": assistant_message_id
                }),
            )?;
        }

        Ok(PromptResponse {
            output: payload.to_string(),
            model: resolved_model,
            events: vec![
                EventFrame::ResponseStart {
                    response_id: response_id.clone(),
                },
                EventFrame::ResponseDelta {
                    response_id: response_id.clone(),
                    delta: "model-selected".to_string(),
                    channel: ResponseChannel::Text,
                },
                EventFrame::ResponseEnd { response_id },
            ],
        })
    }

    /// Evaluates execution policy and dispatches a tool call.
    pub async fn invoke_tool(
        &self,
        call: ToolCall,
        approval_mode: AskForApproval,
        cwd: &Path,
    ) -> Result<Value> {
        let fallback_cwd = cwd.display().to_string();
        let (command, policy_cwd, execution_kind) = call.execution_subject(&fallback_cwd);
        let policy_tool = match &call.payload {
            ToolPayload::LocalShell { .. } => "exec_shell",
            _ => call.name.as_str(),
        };
        let policy_path = permission_path_for_call(&call);
        let decision = self.exec_policy.check(ExecPolicyContext {
            command: &command,
            cwd: &policy_cwd,
            tool: Some(policy_tool),
            path: policy_path.as_deref(),
            ask_for_approval: approval_mode,
            sandbox_mode: None,
        })?;
        let precheck = policy_precheck_payload(&decision, &command, &policy_cwd, execution_kind);
        let response_id = format!("tool-{}", Uuid::new_v4());
        let call_id = call
            .raw_tool_call_id
            .clone()
            .unwrap_or_else(|| format!("tool-call-{}", Uuid::new_v4()));
        self.hooks
            .emit(HookEvent::ToolLifecycle {
                response_id: response_id.clone(),
                tool_name: call.name.clone(),
                phase: "precheck".to_string(),
                payload: precheck.clone(),
            })
            .await;

        if !decision.allow {
            let reason = decision.reason().to_string();
            let approval_id = format!("approval-{}", Uuid::new_v4());
            let error_frame = EventFrame::Error {
                response_id: response_id.clone(),
                message: reason.clone(),
            };
            self.hooks
                .emit(HookEvent::ApprovalLifecycle {
                    approval_id,
                    phase: "denied".to_string(),
                    reason: Some(reason.clone()),
                })
                .await;
            self.hooks
                .emit(HookEvent::GenericEventFrame {
                    frame: Box::new(error_frame.clone()),
                })
                .await;
            return Ok(json!({
                "ok": false,
                "status": "denied",
                "execution_kind": execution_kind,
                "response_id": response_id,
                "precheck": precheck,
                "error": reason,
                "events": [event_frame_payload(&error_frame)],
            }));
        }

        if decision.requires_approval {
            let approval_id = format!("approval-{}", Uuid::new_v4());
            let reason = decision.reason().to_string();
            let maybe_approval_frame = approval_request_frame(
                &decision.requirement,
                decision.matched_rule.as_deref(),
                call_id,
                approval_id.clone(),
                response_id.clone(),
                command.clone(),
                policy_cwd.clone(),
            );
            self.hooks
                .emit(HookEvent::ApprovalLifecycle {
                    approval_id: approval_id.clone(),
                    phase: "requested".to_string(),
                    reason: Some(reason.clone()),
                })
                .await;
            let mut events = Vec::new();
            if let Some(frame) = maybe_approval_frame {
                self.hooks
                    .emit(HookEvent::GenericEventFrame {
                        frame: Box::new(frame.clone()),
                    })
                    .await;
                events.push(event_frame_payload(&frame));
            }
            return Ok(json!({
                "ok": false,
                "status": "approval_required",
                "execution_kind": execution_kind,
                "response_id": response_id,
                "approval_id": approval_id,
                "precheck": precheck,
                "error": reason,
                "events": events,
            }));
        }

        // Headless `request_user_input`: mirror the approval fire-and-return
        // branch (issue #3102). The TUI intercepts this tool by name before
        // dispatch and blocks on a reply channel; the headless runtime instead
        // emits a typed `UserInputRequest` frame and returns a
        // `user_input_required` status so the client can render the question
        // and POST answers back via `AppRequest::SubmitUserInput`. It does NOT
        // block — consistent with the headless approval model, which has no
        // resume channel either.
        if call.name == REQUEST_USER_INPUT_TOOL_NAME {
            let request_id = format!("user-input-{}", Uuid::new_v4());
            let arguments = match &call.payload {
                ToolPayload::Function { arguments } => arguments.as_str(),
                _ => "",
            };
            let maybe_frame = user_input_request_frame(
                call_id.clone(),
                response_id.clone(),
                request_id.clone(),
                arguments,
            );
            let mut events = Vec::new();
            if let Some(frame) = maybe_frame {
                self.hooks
                    .emit(HookEvent::GenericEventFrame {
                        frame: Box::new(frame.clone()),
                    })
                    .await;
                events.push(event_frame_payload(&frame));
            }
            return Ok(json!({
                "ok": false,
                "status": "user_input_required",
                "execution_kind": execution_kind,
                "response_id": response_id,
                "request_id": request_id,
                "precheck": precheck,
                "events": events,
            }));
        }

        let start_frame = EventFrame::ToolCallStart {
            response_id: response_id.clone(),
            tool_name: call.name.clone(),
            arguments: tool_payload_value(&call.payload),
        };
        self.hooks
            .emit(HookEvent::GenericEventFrame {
                frame: Box::new(start_frame.clone()),
            })
            .await;
        self.hooks
            .emit(HookEvent::ToolLifecycle {
                response_id: response_id.clone(),
                tool_name: call.name.clone(),
                phase: "dispatching".to_string(),
                payload: json!({
                    "call_id": call_id,
                    "execution_kind": execution_kind
                }),
            })
            .await;

        match self.tool_registry.dispatch(call.clone(), true).await {
            Ok(tool_output) => {
                let result_frame = EventFrame::ToolCallResult {
                    response_id: response_id.clone(),
                    tool_name: call.name.clone(),
                    output: tool_output_value(&tool_output),
                };
                self.hooks
                    .emit(HookEvent::GenericEventFrame {
                        frame: Box::new(result_frame.clone()),
                    })
                    .await;
                self.hooks
                    .emit(HookEvent::ToolLifecycle {
                        response_id: response_id.clone(),
                        tool_name: call.name,
                        phase: "completed".to_string(),
                        payload: json!({ "ok": true }),
                    })
                    .await;
                Ok(json!({
                    "ok": true,
                    "status": "completed",
                    "execution_kind": execution_kind,
                    "response_id": response_id,
                    "precheck": precheck,
                    "output": tool_output,
                    "events": [
                        event_frame_payload(&start_frame),
                        event_frame_payload(&result_frame)
                    ]
                }))
            }
            Err(err) => {
                let message = format!("{err:?}");
                let error_frame = EventFrame::Error {
                    response_id: response_id.clone(),
                    message: message.clone(),
                };
                self.hooks
                    .emit(HookEvent::GenericEventFrame {
                        frame: Box::new(error_frame.clone()),
                    })
                    .await;
                self.hooks
                    .emit(HookEvent::ToolLifecycle {
                        response_id: response_id.clone(),
                        tool_name: call.name,
                        phase: "failed".to_string(),
                        payload: json!({ "error": message.clone() }),
                    })
                    .await;
                Ok(json!({
                    "ok": false,
                    "status": "failed",
                    "execution_kind": execution_kind,
                    "response_id": response_id,
                    "precheck": precheck,
                    "error": message,
                    "events": [
                        event_frame_payload(&start_frame),
                        event_frame_payload(&error_frame)
                    ]
                }))
            }
        }
    }

    /// Starts all configured MCP servers and emits startup events via hooks.
    pub async fn mcp_startup(&self) -> McpStartupCompleteEvent {
        let mut updates = Vec::new();
        let summary = self.mcp_manager.start_all(|update| {
            updates.push(update);
        });
        for update in updates {
            let status = match update.status {
                McpManagerStartupStatus::Starting => mimofan_protocol::McpStartupStatus::Starting,
                McpManagerStartupStatus::Ready => mimofan_protocol::McpStartupStatus::Ready,
                McpManagerStartupStatus::Failed { error } => {
                    mimofan_protocol::McpStartupStatus::Failed { error }
                }
                McpManagerStartupStatus::Cancelled => mimofan_protocol::McpStartupStatus::Cancelled,
            };
            self.hooks
                .emit(HookEvent::GenericEventFrame {
                    frame: Box::new(EventFrame::McpStartupUpdate {
                        update: mimofan_protocol::McpStartupUpdateEvent {
                            server_name: update.server_name,
                            status,
                        },
                    }),
                })
                .await;
        }
        self.hooks
            .emit(HookEvent::GenericEventFrame {
                frame: Box::new(EventFrame::McpStartupComplete {
                    summary: mimofan_protocol::McpStartupCompleteEvent {
                        ready: summary.ready.clone(),
                        failed: summary
                            .failed
                            .iter()
                            .map(|f| mimofan_protocol::McpStartupFailure {
                                server_name: f.server_name.clone(),
                                error: f.error.clone(),
                            })
                            .collect(),
                        cancelled: summary.cancelled.clone(),
                    },
                }),
            })
            .await;
        summary
    }

    /// Returns the current application status including all jobs and their history.
    pub fn app_status(&self) -> AppResponse {
        let jobs = self.jobs.list();
        let events = jobs
            .iter()
            .flat_map(|job| {
                job.history.iter().map(|entry| EventFrame::ResponseDelta {
                    response_id: job.id.clone(),
                    delta: json!({
                        "kind": "job_transition",
                        "job_id": job.id.clone(),
                        "phase": entry.phase.clone(),
                        "status": job_status_to_str(entry.status),
                        "progress": entry.progress,
                        "detail": entry.detail.clone(),
                        "retry": job_retry_to_value(&entry.retry),
                        "at": entry.at
                    })
                    .to_string(),
                    channel: ResponseChannel::Text,
                })
            })
            .collect::<Vec<_>>();
        AppResponse {
            ok: true,
            data: json!({
                "jobs": jobs.into_iter().map(|job| {
                    json!({
                        "id": job.id,
                        "name": job.name,
                        "status": job_status_to_str(job.status),
                        "progress": job.progress,
                        "detail": job.detail,
                        "retry": job_retry_to_value(&job.retry),
                        "history": job.history.iter().map(job_history_to_value).collect::<Vec<_>>()
                    })
                }).collect::<Vec<_>>()
            }),
            events,
        }
    }

    /// Returns the default model provider from the resolved configuration.
    pub fn provider_default(&self) -> ProviderKind {
        self.config.provider
    }

    /// Saves a named checkpoint for a thread.
    pub fn save_thread_checkpoint(
        &self,
        thread_id: &str,
        checkpoint_id: &str,
        state: &Value,
    ) -> Result<()> {
        self.thread_manager
            .state_store()
            .save_checkpoint(thread_id, checkpoint_id, state)
    }

    /// Loads a checkpoint for a thread. Pass `None` for the latest.
    pub fn load_thread_checkpoint(
        &self,
        thread_id: &str,
        checkpoint_id: Option<&str>,
    ) -> Result<Option<Value>> {
        Ok(self
            .thread_manager
            .state_store()
            .load_checkpoint(thread_id, checkpoint_id)?
            .map(|checkpoint| checkpoint.state))
    }

    /// Enqueues a new background job and persists it immediately.
    pub fn enqueue_job(&mut self, name: impl Into<String>) -> Result<JobRecord> {
        let job = self.jobs.enqueue(name);
        self.jobs
            .persist_job(self.thread_manager.state_store(), &job.id)?;
        Ok(job)
    }

    /// Transitions a job to running and persists the change.
    pub fn set_job_running(&mut self, job_id: &str) -> Result<()> {
        self.jobs.set_running(job_id);
        self.jobs
            .persist_job(self.thread_manager.state_store(), job_id)
    }

    /// Updates a job's progress and persists the change.
    pub fn update_job_progress(
        &mut self,
        job_id: &str,
        progress: u8,
        detail: Option<String>,
    ) -> Result<()> {
        self.jobs.update_progress(job_id, progress, detail);
        self.jobs
            .persist_job(self.thread_manager.state_store(), job_id)
    }

    /// Marks a job as completed and persists the change.
    pub fn complete_job(&mut self, job_id: &str) -> Result<()> {
        self.jobs.complete(job_id);
        self.jobs
            .persist_job(self.thread_manager.state_store(), job_id)
    }

    /// Marks a job as failed and persists the change.
    pub fn fail_job(&mut self, job_id: &str, detail: impl Into<String>) -> Result<()> {
        self.jobs.fail(job_id, detail);
        self.jobs
            .persist_job(self.thread_manager.state_store(), job_id)
    }

    /// Cancels a job and persists the change.
    pub fn cancel_job(&mut self, job_id: &str) -> Result<()> {
        self.jobs.cancel(job_id);
        self.jobs
            .persist_job(self.thread_manager.state_store(), job_id)
    }

    /// Pauses a job and persists the change.
    pub fn pause_job(&mut self, job_id: &str, detail: Option<String>) -> Result<()> {
        self.jobs.pause(job_id, detail);
        self.jobs
            .persist_job(self.thread_manager.state_store(), job_id)
    }

    /// Resumes a paused job and persists the change.
    pub fn resume_job(&mut self, job_id: &str, detail: Option<String>) -> Result<()> {
        self.jobs.resume(job_id, detail);
        self.jobs
            .persist_job(self.thread_manager.state_store(), job_id)
    }

    /// Returns the state-transition history for a job.
    pub fn job_history(&self, job_id: &str) -> Vec<JobHistoryEntry> {
        self.jobs.history(job_id)
    }
}

// ── Helper functions ──────────────────────────────────────────────────

fn thread_response_from_new(status: &str, new: NewThread) -> ThreadResponse {
    ThreadResponse {
        thread_id: new.thread.id.clone(),
        status: status.to_string(),
        thread: Some(new.thread),
        threads: Vec::new(),
        goal: None,
        model: Some(new.model),
        model_provider: Some(new.model_provider),
        cwd: Some(new.cwd),
        approval_policy: new.approval_policy,
        sandbox: new.sandbox,
        events: Vec::new(),
        data: json!({}),
    }
}

fn permission_path_for_call(call: &ToolCall) -> Option<String> {
    match &call.payload {
        ToolPayload::Function { arguments } => serde_json::from_str::<Value>(arguments)
            .ok()
            .and_then(|value| {
                value
                    .get("path")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            }),
        ToolPayload::Mcp { raw_arguments, .. } => raw_arguments
            .get("path")
            .and_then(Value::as_str)
            .map(str::to_string),
        ToolPayload::Custom { .. } | ToolPayload::LocalShell { .. } => None,
    }
}

fn approval_request_frame(
    requirement: &ExecApprovalRequirement,
    matched_rule: Option<&str>,
    call_id: String,
    approval_id: String,
    turn_id: String,
    command: String,
    cwd: String,
) -> Option<EventFrame> {
    let ExecApprovalRequirement::NeedsApproval {
        reason,
        proposed_execpolicy_amendment,
        proposed_network_policy_amendments,
    } = requirement
    else {
        return None;
    };

    let mut available_decisions = vec![
        ReviewDecision::Approved,
        ReviewDecision::ApprovedForSession,
        ReviewDecision::Denied,
        ReviewDecision::Abort,
    ];
    if proposed_execpolicy_amendment
        .as_ref()
        .is_some_and(|amendment| !amendment.prefixes.is_empty())
    {
        available_decisions.push(ReviewDecision::ApprovedExecpolicyAmendment);
    }
    available_decisions.extend(proposed_network_policy_amendments.iter().cloned().map(
        |amendment| ReviewDecision::NetworkPolicyAmendment {
            host: amendment.host,
            action: amendment.action,
        },
    ));

    Some(EventFrame::ExecApprovalRequest {
        request: ExecApprovalRequestEvent {
            call_id,
            approval_id,
            turn_id,
            command,
            cwd,
            reason: reason.clone(),
            matched_rule: matched_rule.map(|rule| rule.to_string().into_boxed_str()),
            network_approval_context: None,
            proposed_execpolicy_amendment: proposed_execpolicy_amendment
                .as_ref()
                .map(|amendment| amendment.prefixes.clone())
                .unwrap_or_default(),
            proposed_network_policy_amendments: proposed_network_policy_amendments.clone(),
            additional_permissions: Vec::new(),
            available_decisions,
        },
    })
}

/// Build an [`EventFrame::UserInputRequest`] for a headless
/// `request_user_input` tool call, mirroring [`approval_request_frame`].
fn user_input_request_frame(
    call_id: String,
    turn_id: String,
    request_id: String,
    arguments: &str,
) -> Option<EventFrame> {
    let parsed: Value = serde_json::from_str(arguments).ok()?;
    let questions = parsed.get("questions").cloned().filter(Value::is_array)?;
    let request = UserInputRequestEvent {
        call_id,
        turn_id,
        request_id,
        questions: serde_json::from_value(questions).ok()?,
    };
    Some(EventFrame::UserInputRequest { request })
}

fn approval_requirement_payload(requirement: &ExecApprovalRequirement) -> Value {
    match requirement {
        ExecApprovalRequirement::Skip {
            bypass_sandbox,
            proposed_execpolicy_amendment,
        } => json!({
            "type": "skip",
            "bypass_sandbox": bypass_sandbox,
            "reason": requirement.reason(),
            "proposed_execpolicy_amendment": proposed_execpolicy_amendment
                .as_ref()
                .map(|amendment| amendment.prefixes.clone())
                .unwrap_or_default()
        }),
        ExecApprovalRequirement::NeedsApproval {
            reason,
            proposed_execpolicy_amendment,
            proposed_network_policy_amendments,
        } => json!({
            "type": "needs_approval",
            "reason": reason,
            "proposed_execpolicy_amendment": proposed_execpolicy_amendment
                .as_ref()
                .map(|amendment| amendment.prefixes.clone())
                .unwrap_or_default(),
            "proposed_network_policy_amendments": proposed_network_policy_amendments
        }),
        ExecApprovalRequirement::Forbidden { reason } => json!({
            "type": "forbidden",
            "reason": reason
        }),
    }
}

fn policy_precheck_payload(
    decision: &ExecPolicyDecision,
    command: &str,
    cwd: &str,
    execution_kind: &str,
) -> Value {
    json!({
        "execution_kind": execution_kind,
        "command": command,
        "cwd": cwd,
        "allow": decision.allow,
        "requires_approval": decision.requires_approval,
        "matched_rule": decision.matched_rule.clone(),
        "phase": decision.requirement.phase(),
        "reason": decision.reason(),
        "requirement": approval_requirement_payload(&decision.requirement)
    })
}

fn tool_payload_value(payload: &ToolPayload) -> Value {
    serde_json::to_value(payload).unwrap_or_else(
        |_| json!({"type":"serialization_error","message":"tool payload unavailable"}),
    )
}

fn tool_output_value(output: &mimofan_protocol::ToolOutput) -> Value {
    serde_json::to_value(output).unwrap_or_else(
        |_| json!({"type":"serialization_error","message":"tool output unavailable"}),
    )
}

fn event_frame_payload(frame: &EventFrame) -> Value {
    serde_json::to_value(frame)
        .unwrap_or_else(|_| json!({"event":"error","message":"failed to encode event frame"}))
}

/// Tool name that triggers the headless clarification-question flow.
const REQUEST_USER_INPUT_TOOL_NAME: &str = "request_user_input";

#[cfg(test)]
mod tests {
    use super::*;
    use mimofan_state::{SessionSource, ThreadMetadata, ThreadStatus as PersistedThreadStatus};
    use mimofan_tools::ToolCallSource;
    use std::path::PathBuf;

    fn temp_core_state(name: &str) -> StateStore {
        let dir =
            std::env::temp_dir().join(format!("mimofan-core-{name}-{}", Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).expect("create temp state dir");
        StateStore::open(Some(dir.join("state.db"))).expect("open state store")
    }

    fn test_thread_metadata(id: &str) -> ThreadMetadata {
        ThreadMetadata {
            id: id.to_string(),
            rollout_path: None,
            preview: "test thread".to_string(),
            ephemeral: false,
            model_provider: "deepseek".to_string(),
            created_at: 10,
            updated_at: 10,
            status: PersistedThreadStatus::Running,
            path: None,
            cwd: PathBuf::from("/tmp/mimo"),
            cli_version: "0.0.0-test".to_string(),
            source: SessionSource::Interactive,
            name: None,
            sandbox_policy: None,
            approval_mode: None,
            archived: false,
            archived_at: None,
            git_sha: None,
            git_branch: None,
            git_origin_url: None,
            memory_mode: None,
            current_leaf_id: None,
        }
    }

    #[test]
    fn permission_path_for_call_extracts_function_path_argument() {
        let call = ToolCall {
            name: "read_file".to_string(),
            payload: ToolPayload::Function {
                arguments: json!({ "path": "README.md" }).to_string(),
            },
            source: ToolCallSource::Direct,
            raw_tool_call_id: None,
        };

        assert_eq!(
            permission_path_for_call(&call).as_deref(),
            Some("README.md")
        );
    }

    #[test]
    fn permission_path_for_call_extracts_mcp_path_argument() {
        let call = ToolCall {
            name: "mcp_fs_read".to_string(),
            payload: ToolPayload::Mcp {
                server: "fs".to_string(),
                tool: "read".to_string(),
                raw_arguments: json!({ "path": "secrets/token.txt" }),
                raw_tool_call_id: None,
            },
            source: ToolCallSource::Direct,
            raw_tool_call_id: None,
        };

        assert_eq!(
            permission_path_for_call(&call).as_deref(),
            Some("secrets/token.txt")
        );
    }

    #[test]
    fn permission_path_for_call_ignores_shell_payload() {
        let call = ToolCall {
            name: "exec_shell".to_string(),
            payload: ToolPayload::LocalShell {
                params: mimofan_protocol::LocalShellParams {
                    command: "cargo test".to_string(),
                    cwd: None,
                    timeout_ms: None,
                },
            },
            source: ToolCallSource::Direct,
            raw_tool_call_id: None,
        };

        assert_eq!(permission_path_for_call(&call), None);
    }

    #[test]
    fn thread_goal_progress_accumulates_durable_accounting() {
        let store = temp_core_state("thread-goal-progress");
        store
            .upsert_thread(&test_thread_metadata("thread-1"))
            .expect("upsert thread");
        let mut manager = ThreadManager::new(store);
        manager
            .set_thread_goal(&ThreadGoalSetParams {
                thread_id: "thread-1".to_string(),
                objective: "Carry the goal across turns".to_string(),
                token_budget: Some(2_000),
            })
            .expect("set goal")
            .expect("goal exists");

        let updated = manager
            .record_thread_goal_progress(&ThreadGoalProgressParams {
                thread_id: "thread-1".to_string(),
                token_delta: 750,
                time_delta_seconds: 12,
                record_continuation: true,
            })
            .expect("record progress")
            .expect("goal exists");

        assert_eq!(updated.tokens_used, 750);
        assert_eq!(updated.time_used_seconds, 12);
        assert_eq!(updated.continuation_count, 1);

        let persisted = manager
            .get_thread_goal(&ThreadGoalGetParams {
                thread_id: "thread-1".to_string(),
            })
            .expect("read goal")
            .expect("goal exists");
        assert_eq!(persisted.tokens_used, 750);
        assert_eq!(persisted.time_used_seconds, 12);
        assert_eq!(persisted.continuation_count, 1);
    }

    #[test]
    fn approval_request_frame_includes_matched_rule() {
        let requirement = ExecApprovalRequirement::NeedsApproval {
            reason: "Typed ask rule 'tool=exec_shell command=cargo test' requires approval."
                .to_string(),
            proposed_execpolicy_amendment: None,
            proposed_network_policy_amendments: Vec::new(),
        };

        let frame = approval_request_frame(
            &requirement,
            Some("tool=exec_shell command=cargo test"),
            "call-1".to_string(),
            "approval-1".to_string(),
            "turn-1".to_string(),
            "cargo test --workspace".to_string(),
            "/repo".to_string(),
        )
        .expect("approval frame");

        let EventFrame::ExecApprovalRequest { request } = frame else {
            panic!("expected exec approval request frame");
        };
        assert_eq!(
            request.matched_rule.as_deref(),
            Some("tool=exec_shell command=cargo test")
        );
        assert_eq!(request.reason, requirement.reason());
    }

    #[test]
    fn user_input_request_frame_lifts_questions_from_arguments() {
        let arguments = r#"{"questions":[{"header":"Scope","id":"scope","question":"Which?","options":[{"label":"A","description":"a"},{"label":"B","description":"b"}],"allow_free_text":true}]}"#;
        let frame = user_input_request_frame(
            "call-1".to_string(),
            "turn-1".to_string(),
            "ui-1".to_string(),
            arguments,
        )
        .expect("user input frame");

        let EventFrame::UserInputRequest { request } = frame else {
            panic!("expected user_input_request frame");
        };
        assert_eq!(request.call_id, "call-1");
        assert_eq!(request.turn_id, "turn-1");
        assert_eq!(request.request_id, "ui-1");
        assert_eq!(request.questions.len(), 1);
        assert_eq!(request.questions[0].id, "scope");
        assert!(request.questions[0].allow_free_text);
        assert!(!request.questions[0].multi_select);
        assert_eq!(request.questions[0].options.len(), 2);
    }

    #[test]
    fn user_input_request_frame_returns_none_on_invalid_arguments() {
        let frame = user_input_request_frame(
            "call-1".to_string(),
            "turn-1".to_string(),
            "ui-1".to_string(),
            "not json",
        );
        assert!(frame.is_none());

        let frame = user_input_request_frame(
            "call-1".to_string(),
            "turn-1".to_string(),
            "ui-1".to_string(),
            r#"{"foo":"bar"}"#,
        );
        assert!(frame.is_none());
    }
}
