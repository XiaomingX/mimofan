//! Thread management route handlers.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;

use mimofan_protocol::runtime::DynamicToolCallResult;

use crate::runtime_threads::{
    CompactThreadRequest, CreateThreadRequest, ExternalApprovalDecision, StartTurnRequest,
    SteerTurnRequest, ThreadRecord, UpdateThreadRequest, UsageGroupBy,
};

use super::RuntimeApiState;
use super::types::{
    ApiError, DecideApprovalBody, DecideApprovalResponse, PatchUndoResponse, PatchUndoResult,
    RetryTurnRequest, RetryTurnResponse, StartTurnResponse, SubmitUserInputBody,
    SubmitUserInputResponse, ThreadSummary, ThreadSummaryQuery, ThreadsQuery, UndoTurnRequest,
    UndoTurnResponse,
};

pub(crate) async fn create_task(
    State(state): State<RuntimeApiState>,
    Json(mut req): Json<crate::task_manager::NewTaskRequest>,
) -> Result<(StatusCode, Json<crate::task_manager::TaskRecord>), ApiError> {
    if req.prompt.trim().is_empty() {
        return Err(ApiError::bad_request("prompt is required"));
    }
    if req.workspace.is_none() {
        req.workspace = Some(state.workspace.clone());
    }
    if req.model.is_none() {
        req.model = Some(state.config.default_model());
    }
    let task = state
        .task_manager
        .add_task(req)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    Ok((StatusCode::CREATED, Json(task)))
}

pub(crate) async fn create_thread(
    State(state): State<RuntimeApiState>,
    Json(mut req): Json<CreateThreadRequest>,
) -> Result<(StatusCode, Json<ThreadRecord>), ApiError> {
    if req.model.as_ref().is_none_or(|m| m.trim().is_empty()) {
        req.model = Some(state.config.default_model());
    }
    if req.workspace.is_none() {
        req.workspace = Some(state.workspace.clone());
    }
    if req.mode.as_ref().is_none_or(|m| m.trim().is_empty()) {
        req.mode = Some("agent".to_string());
    }

    let thread = state
        .runtime_threads
        .create_thread(req)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    Ok((StatusCode::CREATED, Json(thread)))
}

pub(crate) async fn list_threads(
    State(state): State<RuntimeApiState>,
    Query(query): Query<ThreadsQuery>,
) -> Result<Json<Vec<ThreadRecord>>, ApiError> {
    let filter = super::types::resolve_thread_filter(query.include_archived, query.archived_only);
    let threads = state
        .runtime_threads
        .list_threads(filter, query.limit)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(threads))
}

pub(crate) async fn list_threads_summary(
    State(state): State<RuntimeApiState>,
    Query(query): Query<ThreadSummaryQuery>,
) -> Result<Json<Vec<ThreadSummary>>, ApiError> {
    let limit = query.limit.unwrap_or(50).clamp(1, 500);
    let search = query.search.as_deref().map(str::to_ascii_lowercase);
    let filter = super::types::resolve_thread_filter(query.include_archived, query.archived_only);
    let threads = state
        .runtime_threads
        .list_threads(filter, Some(limit))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let mut summaries = Vec::new();
    for thread in threads {
        let detail = state
            .runtime_threads
            .get_thread_detail(&thread.id)
            .await
            .map_err(super::types::map_thread_err)?;
        let latest_turn = detail.turns.last();
        let latest_status =
            latest_turn.map(|turn| format!("{:?}", turn.status).to_ascii_lowercase());

        let title = thread
            .title
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(|t| super::types::truncate_text(t, 72))
            .unwrap_or_else(|| {
                latest_turn
                    .map(|turn| {
                        if turn.input_summary.trim().is_empty() {
                            "New Thread".to_string()
                        } else {
                            super::types::truncate_text(&turn.input_summary, 72)
                        }
                    })
                    .unwrap_or_else(|| "New Thread".to_string())
            });

        let preview = detail
            .items
            .iter()
            .rev()
            .find_map(|item| match item.kind {
                crate::runtime_threads::TurnItemKind::AgentMessage
                | crate::runtime_threads::TurnItemKind::UserMessage => {
                    let text = item.detail.clone().unwrap_or_else(|| item.summary.clone());
                    if text.trim().is_empty() {
                        None
                    } else {
                        Some(super::types::truncate_text(&text, 140))
                    }
                }
                _ => None,
            })
            .unwrap_or_else(|| title.clone());

        if let Some(search) = &search {
            let haystack = format!(
                "{} {} {} {}",
                thread.id.to_ascii_lowercase(),
                title.to_ascii_lowercase(),
                preview.to_ascii_lowercase(),
                thread.model.to_ascii_lowercase()
            );
            if !haystack.contains(search) {
                continue;
            }
        }

        let workspace_git =
            super::workspace_routes::collect_workspace_git_metadata(&thread.workspace);
        summaries.push(ThreadSummary {
            id: thread.id,
            title,
            preview,
            model: thread.model,
            mode: thread.mode,
            branch: workspace_git.branch,
            head: workspace_git.head,
            dirty: workspace_git.dirty,
            workspace: thread.workspace,
            archived: thread.archived,
            updated_at: thread.updated_at,
            latest_turn_id: thread.latest_turn_id,
            latest_turn_status: latest_status,
        });
    }

    if summaries.len() > limit {
        summaries.truncate(limit);
    }

    Ok(Json(summaries))
}

pub(crate) async fn get_thread(
    State(state): State<RuntimeApiState>,
    Path(id): Path<String>,
) -> Result<Json<crate::runtime_threads::ThreadDetail>, ApiError> {
    let detail = state
        .runtime_threads
        .get_thread_detail(&id)
        .await
        .map_err(super::types::map_thread_err)?;
    Ok(Json(detail))
}

pub(crate) async fn update_thread(
    State(state): State<RuntimeApiState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateThreadRequest>,
) -> Result<Json<ThreadRecord>, ApiError> {
    let thread = state
        .runtime_threads
        .update_thread(&id, req)
        .await
        .map_err(super::types::map_thread_err)?;
    Ok(Json(thread))
}

pub(crate) async fn resume_thread(
    State(state): State<RuntimeApiState>,
    Path(id): Path<String>,
) -> Result<Json<ThreadRecord>, ApiError> {
    let thread = state
        .runtime_threads
        .resume_thread(&id)
        .await
        .map_err(super::types::map_thread_err)?;
    Ok(Json(thread))
}

pub(crate) async fn fork_thread(
    State(state): State<RuntimeApiState>,
    Path(id): Path<String>,
) -> Result<(StatusCode, Json<ThreadRecord>), ApiError> {
    let thread = state
        .runtime_threads
        .fork_thread(&id)
        .await
        .map_err(super::types::map_thread_err)?;
    Ok((StatusCode::CREATED, Json(thread)))
}

pub(crate) async fn undo_thread_turn(
    State(state): State<RuntimeApiState>,
    Path(id): Path<String>,
    Json(req): Json<UndoTurnRequest>,
) -> Result<(StatusCode, Json<UndoTurnResponse>), ApiError> {
    let depth = req.depth.unwrap_or(0);
    let (forked_thread, original_user_text) = state
        .runtime_threads
        .fork_at_user_message(&id, depth)
        .await
        .map_err(super::types::map_thread_err)?;
    Ok((
        StatusCode::CREATED,
        Json(UndoTurnResponse {
            thread: forked_thread,
            original_user_text,
        }),
    ))
}

pub(crate) async fn patch_undo_thread_turn(
    State(state): State<RuntimeApiState>,
    Path(id): Path<String>,
    Json(req): Json<UndoTurnRequest>,
) -> Result<(StatusCode, Json<PatchUndoResponse>), ApiError> {
    let depth = req.depth.unwrap_or(0);

    // Step 1: Try snapshot-based file rollback (patch_undo).
    let thread = state
        .runtime_threads
        .get_thread(&id)
        .await
        .map_err(super::types::map_thread_err)?;
    let patch_result = patch_undo_workspace_files(&thread.workspace);

    // Step 2: Remove the last conversation turn (undo_conversation).
    let (forked_thread, original_user_text) = state
        .runtime_threads
        .fork_at_user_message(&id, depth)
        .await
        .map_err(super::types::map_thread_err)?;

    Ok((
        StatusCode::CREATED,
        Json(PatchUndoResponse {
            patch_result,
            thread: forked_thread,
            original_user_text,
        }),
    ))
}

pub(crate) async fn retry_thread_turn(
    State(state): State<RuntimeApiState>,
    Path(id): Path<String>,
    Json(req): Json<RetryTurnRequest>,
) -> Result<(StatusCode, Json<RetryTurnResponse>), ApiError> {
    let depth = req.depth.unwrap_or(0);
    let (forked_thread, original_user_text) = state
        .runtime_threads
        .fork_at_user_message(&id, depth)
        .await
        .map_err(super::types::map_thread_err)?;

    let retry_prompt = req.prompt.or(original_user_text).unwrap_or_default();
    if retry_prompt.trim().is_empty() {
        return Err(ApiError::bad_request(
            "No user message to retry — the dropped turn had no user text",
        ));
    }

    let turn = state
        .runtime_threads
        .start_turn(
            &forked_thread.id,
            StartTurnRequest {
                prompt: retry_prompt,
                input_summary: None,
                model: None,
                mode: None,
                allow_shell: None,
                trust_mode: None,
                auto_approve: None,
                dynamic_tools: Vec::new(),
                environment_id: None,
                response_format: None,
            },
        )
        .await
        .map_err(super::types::map_thread_err)?;

    Ok((
        StatusCode::CREATED,
        Json(RetryTurnResponse {
            thread: forked_thread,
            turn,
        }),
    ))
}

pub(crate) async fn start_thread_turn(
    State(state): State<RuntimeApiState>,
    Path(id): Path<String>,
    Json(req): Json<StartTurnRequest>,
) -> Result<(StatusCode, Json<StartTurnResponse>), ApiError> {
    let turn = state
        .runtime_threads
        .start_turn(&id, req)
        .await
        .map_err(super::types::map_thread_err)?;
    let thread = state
        .runtime_threads
        .get_thread(&id)
        .await
        .map_err(super::types::map_thread_err)?;
    Ok((
        StatusCode::CREATED,
        Json(StartTurnResponse { thread, turn }),
    ))
}

pub(crate) async fn steer_thread_turn(
    State(state): State<RuntimeApiState>,
    Path((id, turn_id)): Path<(String, String)>,
    Json(req): Json<SteerTurnRequest>,
) -> Result<Json<crate::runtime_threads::TurnRecord>, ApiError> {
    let turn = state
        .runtime_threads
        .steer_turn(&id, &turn_id, req)
        .await
        .map_err(super::types::map_thread_err)?;
    Ok(Json(turn))
}

pub(crate) async fn interrupt_thread_turn(
    State(state): State<RuntimeApiState>,
    Path((id, turn_id)): Path<(String, String)>,
) -> Result<Json<crate::runtime_threads::TurnRecord>, ApiError> {
    let turn = state
        .runtime_threads
        .interrupt_turn(&id, &turn_id)
        .await
        .map_err(super::types::map_thread_err)?;
    Ok(Json(turn))
}

pub(crate) async fn deliver_dynamic_tool_result(
    State(state): State<RuntimeApiState>,
    Path((id, _turn_id, call_id)): Path<(String, String, String)>,
    Json(result): Json<DynamicToolCallResult>,
) -> Result<StatusCode, ApiError> {
    state
        .runtime_threads
        .get_thread(&id)
        .await
        .map_err(super::types::map_thread_err)?;
    if state
        .runtime_threads
        .deliver_dynamic_tool_result(&call_id, result)
    {
        Ok(StatusCode::ACCEPTED)
    } else {
        Err(ApiError::not_found(format!(
            "No pending dynamic tool call '{call_id}'"
        )))
    }
}

pub(crate) async fn compact_thread(
    State(state): State<RuntimeApiState>,
    Path(id): Path<String>,
    Json(req): Json<CompactThreadRequest>,
) -> Result<(StatusCode, Json<StartTurnResponse>), ApiError> {
    let turn = state
        .runtime_threads
        .compact_thread(&id, req)
        .await
        .map_err(super::types::map_thread_err)?;
    let thread = state
        .runtime_threads
        .get_thread(&id)
        .await
        .map_err(super::types::map_thread_err)?;
    Ok((
        StatusCode::ACCEPTED,
        Json(StartTurnResponse { thread, turn }),
    ))
}

pub(crate) async fn decide_approval(
    State(state): State<RuntimeApiState>,
    Path(approval_id): Path<String>,
    Json(req): Json<DecideApprovalBody>,
) -> Result<Json<DecideApprovalResponse>, ApiError> {
    let decision = match req.decision.as_str() {
        "allow" => ExternalApprovalDecision::Allow {
            remember: req.remember,
        },
        "deny" => ExternalApprovalDecision::Deny {
            remember: req.remember,
        },
        other => {
            return Err(ApiError::bad_request(format!(
                "invalid decision '{other}'; expected \"allow\" or \"deny\""
            )));
        }
    };
    let delivered = state
        .runtime_threads
        .deliver_external_approval(&approval_id, decision);
    if !delivered {
        return Err(ApiError::not_found(format!(
            "no pending approval with id '{approval_id}'"
        )));
    }
    Ok(Json(DecideApprovalResponse {
        ok: true,
        approval_id,
        decision: req.decision,
        delivered,
    }))
}

pub(crate) async fn submit_user_input(
    State(state): State<RuntimeApiState>,
    Path((thread_id, input_id)): Path<(String, String)>,
    Json(req): Json<SubmitUserInputBody>,
) -> Result<Json<SubmitUserInputResponse>, ApiError> {
    use crate::tools::user_input::{UserInputAnswer, UserInputResponse};
    let answers: Vec<UserInputAnswer> = req
        .answers
        .into_iter()
        .map(|a| UserInputAnswer {
            id: a.id,
            label: a.label,
            value: a.value,
        })
        .collect();
    let response = UserInputResponse { answers };
    let delivered = state
        .runtime_threads
        .submit_user_input(&thread_id, &input_id, response)
        .await
        .map_err(super::types::map_thread_err)?;
    Ok(Json(SubmitUserInputResponse {
        ok: true,
        input_id,
        delivered,
    }))
}

pub(crate) async fn list_tasks(
    State(state): State<RuntimeApiState>,
    Query(query): Query<super::types::TasksQuery>,
) -> Result<Json<super::types::TasksResponse>, ApiError> {
    let tasks = state.task_manager.list_tasks(query.limit).await;
    let counts = state.task_manager.counts().await;
    Ok(Json(super::types::TasksResponse { tasks, counts }))
}

pub(crate) async fn get_task(
    State(state): State<RuntimeApiState>,
    Path(id): Path<String>,
) -> Result<Json<crate::task_manager::TaskRecord>, ApiError> {
    let task = state
        .task_manager
        .get_task(&id)
        .await
        .map_err(super::types::map_task_err)?;
    Ok(Json(task))
}

pub(crate) async fn cancel_task(
    State(state): State<RuntimeApiState>,
    Path(id): Path<String>,
) -> Result<Json<crate::task_manager::TaskRecord>, ApiError> {
    let task = state
        .task_manager
        .cancel_task(&id)
        .await
        .map_err(super::types::map_task_err)?;
    Ok(Json(task))
}

pub(crate) async fn get_usage(
    State(state): State<RuntimeApiState>,
    Query(query): Query<super::types::UsageQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let since = match query.since.as_deref() {
        Some(raw) => Some(super::types::parse_iso8601(raw, "since")?),
        None => None,
    };
    let until = match query.until.as_deref() {
        Some(raw) => Some(super::types::parse_iso8601(raw, "until")?),
        None => None,
    };
    if let (Some(s), Some(u)) = (since, until)
        && s > u
    {
        return Err(ApiError::bad_request("since must be <= until".to_string()));
    }
    let group_by = match query.group_by.as_deref().unwrap_or("day") {
        "day" => UsageGroupBy::Day,
        "model" => UsageGroupBy::Model,
        "provider" => UsageGroupBy::Provider,
        "thread" => UsageGroupBy::Thread,
        other => {
            return Err(ApiError::bad_request(format!(
                "Unsupported group_by '{other}': expected one of day, model, provider, thread"
            )));
        }
    };

    let aggregation = state
        .runtime_threads
        .aggregate_usage(since, until, group_by)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(serde_json::json!(aggregation)))
}

pub(crate) async fn list_snapshots(
    State(state): State<RuntimeApiState>,
    Query(query): Query<super::types::SnapshotsQuery>,
) -> Result<Json<Vec<super::types::SnapshotEntry>>, ApiError> {
    Ok(Json(snapshot_entries_for_workspace(
        &state.workspace,
        query,
    )?))
}

pub(crate) async fn restore_snapshot(
    State(state): State<RuntimeApiState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    restore_snapshot_for_workspace(&state.workspace, &id)?;
    Ok(Json(serde_json::json!({
        "restored": id,
    })))
}

// ── Helper functions ────────────────────────────────────────────────

/// Restore the newest `tool:` or `pre-turn:` snapshot that differs from the
/// current workspace.
fn patch_undo_workspace_files(workspace: &std::path::Path) -> PatchUndoResult {
    let repo = match crate::snapshot::SnapshotRepo::open_or_init(workspace) {
        Ok(repo) => repo,
        Err(e) => {
            return PatchUndoResult {
                files_restored: false,
                summary: Some(format!("Snapshot repo unavailable: {e}")),
                snapshot_label: None,
            };
        }
    };
    let snapshots = match repo.list(20) {
        Ok(snapshots) => snapshots,
        Err(e) => {
            return PatchUndoResult {
                files_restored: false,
                summary: Some(format!("Failed to list snapshots: {e}")),
                snapshot_label: None,
            };
        }
    };
    let target = snapshots
        .iter()
        .filter(|s| s.label.starts_with("tool:") || s.label.starts_with("pre-turn:"))
        .find(|s| matches!(repo.work_tree_matches_snapshot(&s.id), Ok(false) | Err(_)));
    let Some(target) = target else {
        return PatchUndoResult {
            files_restored: false,
            summary: Some(
                "No older tool or pre-turn snapshots differ from the current workspace."
                    .to_string(),
            ),
            snapshot_label: None,
        };
    };
    if let Err(e) = repo.restore(&target.id) {
        return PatchUndoResult {
            files_restored: false,
            summary: Some(format!("Restore failed: {e}")),
            snapshot_label: None,
        };
    }

    // Compute a diff stat for the summary.
    use crate::dependencies::{ExternalTool as _, Git};
    let diff_stat = Git::command().and_then(|mut git| {
        git.args(["diff", "--stat"])
            .current_dir(workspace)
            .output()
            .ok()
            .and_then(|o| {
                let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                if s.is_empty() { None } else { Some(s) }
            })
    });

    let short = &target.id.as_str()[..target.id.as_str().len().min(8)];
    let summary = match diff_stat {
        Some(ref stat) => format!(
            "Restored snapshot '{}' ({}). Files affected:\n{stat}",
            target.label, short
        ),
        None => format!(
            "Restored snapshot '{}' ({}). No diff changes detected.",
            target.label, short
        ),
    };
    PatchUndoResult {
        files_restored: true,
        summary: Some(summary),
        snapshot_label: Some(target.label.clone()),
    }
}

fn restore_snapshot_for_workspace(workspace: &std::path::Path, id: &str) -> Result<(), ApiError> {
    let repo = crate::snapshot::SnapshotRepo::open_or_init(workspace)
        .map_err(|e| ApiError::internal(format!("Snapshot repo init failed: {e}")))?;
    let snapshot_id = crate::snapshot::SnapshotId(id.to_string());
    repo.restore(&snapshot_id)
        .map_err(|e| ApiError::internal(format!("Snapshot restore failed: {e}")))
}

fn snapshot_entries_for_workspace(
    workspace: &std::path::Path,
    query: super::types::SnapshotsQuery,
) -> Result<Vec<super::types::SnapshotEntry>, ApiError> {
    const DEFAULT_LIMIT: usize = 20;
    const MAX_LIMIT: usize = 100;

    let limit = match query.limit.unwrap_or(DEFAULT_LIMIT) {
        1..=MAX_LIMIT => query.limit.unwrap_or(DEFAULT_LIMIT),
        other => {
            return Err(ApiError::bad_request(format!(
                "limit must be between 1 and {MAX_LIMIT}; got {other}",
            )));
        }
    };
    let repo = crate::snapshot::SnapshotRepo::open_or_init(workspace)
        .map_err(|e| ApiError::internal(format!("Snapshot repo unavailable: {e}")))?;
    let snapshots = repo
        .list(limit)
        .map_err(|e| ApiError::internal(format!("Failed to list snapshots: {e}")))?;
    Ok(snapshots
        .into_iter()
        .map(|snapshot| super::types::SnapshotEntry {
            id: snapshot.id.as_str().to_string(),
            label: snapshot.label,
            timestamp: snapshot.timestamp,
        })
        .collect())
}
