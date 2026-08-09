//! Session management route handlers.

use std::collections::HashMap;

use anyhow::Result;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;

use crate::models::{ContentBlock, Message};
use crate::runtime_threads::requests::{CreateThreadRequest, ThreadDetail};
use crate::runtime_threads::types::{RuntimeTurnStatus, TurnItemKind, TurnItemLifecycleStatus};
use crate::session_manager::{SavedSession, SessionManager, create_saved_session_with_id_and_mode};

use super::RuntimeApiState;
use super::types::{
    ApiError, CreateSessionRequest, CreateSessionResponse, ResumeSessionRequest,
    ResumeSessionResponse, SaveSessionRequest, SaveSessionResponse, SessionDetailResponse,
    SessionsQuery, SessionsResponse,
};

pub(crate) async fn list_sessions(
    State(state): State<RuntimeApiState>,
    Query(query): Query<SessionsQuery>,
) -> Result<Json<SessionsResponse>, ApiError> {
    let manager = SessionManager::new(state.sessions_dir.clone())
        .map_err(|e| ApiError::internal(format!("Failed to open sessions dir: {e}")))?;
    let mut sessions = if let Some(search) = query.search {
        manager
            .search_sessions_fulltext(&search)
            .map_err(|e| ApiError::internal(format!("Failed to search sessions: {e}")))?
            .into_iter()
            .map(|hit| hit.metadata)
            .collect()
    } else {
        manager
            .list_sessions()
            .map_err(|e| ApiError::internal(format!("Failed to list sessions: {e}")))?
    };
    let limit = query.limit.unwrap_or(50).clamp(1, 500);
    sessions.truncate(limit);
    Ok(Json(SessionsResponse { sessions }))
}

pub(crate) async fn get_session(
    State(state): State<RuntimeApiState>,
    Path(id): Path<String>,
) -> Result<Json<SessionDetailResponse>, ApiError> {
    let manager = SessionManager::new(state.sessions_dir.clone())
        .map_err(|e| ApiError::internal(format!("Failed to open sessions dir: {e}")))?;
    let session = manager
        .load_session(&id)
        .map_err(|e| map_session_err(&id, e, "read"))?;
    Ok(Json(session_to_detail(session)))
}

pub(crate) async fn resume_session_thread(
    State(state): State<RuntimeApiState>,
    Path(id): Path<String>,
    Json(req): Json<ResumeSessionRequest>,
) -> Result<(StatusCode, Json<ResumeSessionResponse>), ApiError> {
    let manager = SessionManager::new(state.sessions_dir.clone())
        .map_err(|e| ApiError::internal(format!("Failed to open sessions dir: {e}")))?;
    let session = manager
        .load_session(&id)
        .map_err(|e| map_session_err(&id, e, "read"))?;

    let model = req.model.unwrap_or_else(|| session.metadata.model.clone());
    let mode = req.mode.unwrap_or_else(|| {
        session
            .metadata
            .mode
            .clone()
            .unwrap_or_else(|| "agent".to_string())
    });

    let thread = state
        .runtime_threads
        .create_thread(CreateThreadRequest {
            model: Some(model),
            workspace: Some(state.workspace.clone()),
            mode: Some(mode),
            allow_shell: None,
            trust_mode: None,
            auto_approve: None,
            archived: false,
            system_prompt: session.system_prompt.clone(),
            task_id: None,
            ..Default::default()
        })
        .await
        .map_err(|e| ApiError::internal(format!("Failed to create thread: {e}")))?;

    let msg_count = session.messages.len();
    state
        .runtime_threads
        .seed_thread_from_messages(&thread.id, &session.messages)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to seed thread history: {e}")))?;

    // Link the session to the new thread so that `ensure_engine_loaded`
    // can restore the full message history from the session file.
    if let Err(e) = state
        .runtime_threads
        .set_thread_session_id(&thread.id, &id)
        .await
    {
        let session_ref = crate::utils::redacted_identifier_for_log(&id);
        tracing::warn!(
            session = %session_ref,
            thread_id = %thread.id,
            error = %e,
            "Failed to link session to thread"
        );
    }

    let summary = format!(
        "Resumed session '{}' ({} messages) into thread {}",
        session.metadata.title, msg_count, thread.id
    );

    Ok((
        StatusCode::CREATED,
        Json(ResumeSessionResponse {
            thread_id: thread.id,
            session_id: id,
            message_count: msg_count,
            summary,
        }),
    ))
}

pub(crate) async fn create_session_from_thread(
    State(state): State<RuntimeApiState>,
    Json(req): Json<CreateSessionRequest>,
) -> Result<(StatusCode, Json<CreateSessionResponse>), ApiError> {
    let thread_id = req.thread_id.trim();
    if thread_id.is_empty() {
        return Err(ApiError::bad_request("thread_id is required"));
    }

    let detail = state
        .runtime_threads
        .get_thread_detail(thread_id)
        .await
        .map_err(super::types::map_thread_err)?;

    if thread_detail_has_live_work(&detail) {
        return Err(ApiError {
            status: StatusCode::CONFLICT,
            message: format!(
                "Thread {thread_id} has a queued or active turn; wait for completion before saving as a session"
            ),
        });
    }

    let messages = messages_from_thread_detail(&detail);
    if messages.is_empty() {
        return Err(ApiError::bad_request(format!(
            "Thread {thread_id} has no user or assistant messages to save"
        )));
    }

    let manager = SessionManager::new(state.sessions_dir.clone())
        .map_err(|e| ApiError::internal(format!("Failed to open sessions dir: {e}")))?;
    let total_tokens = total_tokens_from_thread_detail(&detail);
    let session_handle = uuid::Uuid::new_v4().to_string();
    let mut session = create_saved_session_with_id_and_mode(
        session_handle.clone(),
        &messages,
        &detail.thread.model,
        &detail.thread.workspace,
        total_tokens,
        None,
        Some(&detail.thread.mode),
    );
    session.system_prompt = detail.thread.system_prompt.clone();

    if let Some(title) =
        session_title_override(req.title.as_deref(), detail.thread.title.as_deref())
    {
        session.metadata.title = title;
    }
    let title = session.metadata.title.clone();
    let message_count = session.metadata.message_count;

    manager
        .save_session(&session)
        .map_err(|e| ApiError::internal(format!("Failed to save session: {e}")))?;

    // Link the session to the thread so that `ensure_engine_loaded` can
    // restore the full message history from the session file.
    if let Err(e) = state
        .runtime_threads
        .set_thread_session_id(&detail.thread.id, &session_handle)
        .await
    {
        let session_ref = crate::utils::redacted_identifier_for_log(&session_handle);
        tracing::warn!(
            session = %session_ref,
            thread_id = %detail.thread.id,
            error = %e,
            "Failed to link session to thread"
        );
    }

    Ok((
        StatusCode::CREATED,
        Json(CreateSessionResponse {
            session_id: session_handle,
            thread_id: detail.thread.id,
            message_count,
            title,
        }),
    ))
}

/// `PUT /v1/sessions` — save a thread's current engine state as a session.
pub(crate) async fn save_current_session(
    State(state): State<RuntimeApiState>,
    Json(req): Json<SaveSessionRequest>,
) -> Result<Json<SaveSessionResponse>, ApiError> {
    // Find the thread to save.
    let thread_id = match req.thread_id {
        Some(id) => id,
        None => {
            // Find the most recently updated thread.
            let threads = state
                .runtime_threads
                .list_threads(
                    crate::runtime_threads::ThreadListFilter::IncludeArchived,
                    Some(100),
                )
                .await
                .map_err(super::types::map_thread_err)?;
            threads
                .into_iter()
                .max_by_key(|t| t.updated_at)
                .map(|t| t.id)
                .ok_or_else(|| ApiError::bad_request("No threads to save"))?
        }
    };

    let engine = state
        .runtime_threads
        .get_engine(&thread_id)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to get engine for thread: {e}")))?;

    let snapshot = engine
        .get_session_snapshot()
        .await
        .map_err(|e| ApiError::internal(format!("Failed to get session snapshot: {e}")))?;

    let manager = SessionManager::new(state.sessions_dir.clone())
        .map_err(|e| ApiError::internal(format!("Failed to open sessions dir: {e}")))?;

    let session = if let Some(ref existing_id) = req.session_id {
        match manager.load_session(existing_id) {
            Ok(existing) => {
                let mut updated = crate::session_manager::update_session(
                    existing,
                    &snapshot.messages,
                    snapshot.total_tokens,
                    snapshot.system_prompt.as_ref(),
                );
                updated.metadata.model = snapshot.model.clone();
                updated.metadata.mode = Some(snapshot.mode.clone());
                updated
            }
            Err(e) => {
                if e.kind() == std::io::ErrorKind::NotFound {
                    crate::session_manager::create_saved_session_with_id_and_mode(
                        existing_id.clone(),
                        &snapshot.messages,
                        &snapshot.model,
                        &snapshot.workspace,
                        snapshot.total_tokens,
                        snapshot.system_prompt.as_ref(),
                        Some(snapshot.mode.as_str()),
                    )
                } else {
                    return Err(ApiError::internal(format!(
                        "Failed to load session {existing_id}: {e}"
                    )));
                }
            }
        }
    } else {
        crate::session_manager::create_saved_session_with_mode(
            &snapshot.messages,
            &snapshot.model,
            &snapshot.workspace,
            snapshot.total_tokens,
            snapshot.system_prompt.as_ref(),
            Some(snapshot.mode.as_str()),
        )
    };

    manager
        .save_session(&session)
        .map_err(|e| ApiError::internal(format!("Failed to save session: {e}")))?;

    let session_handle = session.metadata.id.clone();
    if let Err(e) = state
        .runtime_threads
        .set_thread_session_id(&thread_id, &session_handle)
        .await
    {
        let session_ref = crate::utils::redacted_identifier_for_log(&session_handle);
        tracing::warn!(
            session = %session_ref,
            thread_id = %thread_id,
            error = %e,
            "Failed to link session to thread"
        );
    }

    Ok(Json(SaveSessionResponse {
        session_id: session_handle,
        session: session_to_detail(session),
    }))
}

pub(crate) async fn delete_session(
    State(state): State<RuntimeApiState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let manager = SessionManager::new(state.sessions_dir.clone())
        .map_err(|e| ApiError::internal(format!("Failed to open sessions dir: {e}")))?;
    manager
        .delete_session(&id)
        .map_err(|e| map_session_err(&id, e, "delete"))?;
    Ok(StatusCode::NO_CONTENT)
}

// ── Helper functions ────────────────────────────────────────────────

pub(crate) fn thread_detail_has_live_work(detail: &ThreadDetail) -> bool {
    detail.turns.iter().any(|turn| {
        matches!(
            turn.status,
            RuntimeTurnStatus::Queued | RuntimeTurnStatus::InProgress
        )
    }) || detail.items.iter().any(|item| {
        matches!(
            item.status,
            TurnItemLifecycleStatus::Queued | TurnItemLifecycleStatus::InProgress
        )
    })
}

pub(crate) fn messages_from_thread_detail(detail: &ThreadDetail) -> Vec<Message> {
    let items_by_id: HashMap<&str, _> = detail
        .items
        .iter()
        .map(|item| (item.id.as_str(), item))
        .collect();
    let mut messages = Vec::new();

    for turn in &detail.turns {
        let mut assistant_blocks: Vec<ContentBlock> = Vec::new();
        let mut user_blocks: Vec<ContentBlock> = Vec::new();
        let flush_assistant = |blocks: &mut Vec<ContentBlock>, msgs: &mut Vec<Message>| {
            if !blocks.is_empty() {
                msgs.push(Message {
                    role: "assistant".to_string(),
                    content: std::mem::take(blocks),
                });
            }
        };
        let flush_user = |blocks: &mut Vec<ContentBlock>, msgs: &mut Vec<Message>| {
            if !blocks.is_empty() {
                msgs.push(Message {
                    role: "user".to_string(),
                    content: std::mem::take(blocks),
                });
            }
        };

        for item_id in &turn.item_ids {
            let Some(item) = items_by_id.get(item_id.as_str()) else {
                continue;
            };
            match item.kind {
                TurnItemKind::UserMessage => {
                    flush_assistant(&mut assistant_blocks, &mut messages);

                    let text = item.detail.as_deref().map(str::trim).unwrap_or("");
                    if !text.is_empty() {
                        user_blocks.push(ContentBlock::Text {
                            text: text.to_string(),
                            cache_control: None,
                        });
                    }
                }
                TurnItemKind::AgentMessage => {
                    flush_user(&mut user_blocks, &mut messages);
                    let text = item.detail.as_deref().map(str::trim).unwrap_or("");
                    if !text.is_empty() {
                        assistant_blocks.push(ContentBlock::Text {
                            text: text.to_string(),
                            cache_control: None,
                        });
                    }
                }
                TurnItemKind::AgentReasoning => {
                    flush_user(&mut user_blocks, &mut messages);
                    let thinking = item.detail.as_deref().map(str::trim).unwrap_or("");
                    if !thinking.is_empty() {
                        assistant_blocks.push(ContentBlock::Thinking {
                            thinking: thinking.to_string(),
                            signature: None,
                        });
                    }
                }
                TurnItemKind::ToolCall => {
                    let meta = item.metadata.as_ref();
                    let is_tool_result = meta.and_then(|m| m.get("tool_result_for")).is_some();
                    if is_tool_result {
                        flush_assistant(&mut assistant_blocks, &mut messages);

                        let tool_use_id = meta
                            .and_then(|m| m.get("tool_result_for"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let content = item.detail.as_deref().unwrap_or("").to_string();
                        let is_error = meta
                            .and_then(|m| m.get("is_error"))
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);
                        let content_blocks = meta
                            .and_then(|m| m.get("content_blocks"))
                            .and_then(|v| v.as_array())
                            .cloned();
                        user_blocks.push(ContentBlock::ToolResult {
                            tool_use_id,
                            content,
                            is_error: if is_error { Some(true) } else { None },
                            content_blocks,
                        });
                    } else {
                        flush_user(&mut user_blocks, &mut messages);
                        let tool_use_id = meta
                            .and_then(|m| m.get("tool_use_id"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let tool_name = meta
                            .and_then(|m| m.get("tool_name"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let input_str = item.detail.as_deref().unwrap_or("{}");
                        let input: serde_json::Value =
                            serde_json::from_str(input_str).unwrap_or(serde_json::Value::Null);
                        assistant_blocks.push(ContentBlock::ToolUse {
                            id: tool_use_id,
                            name: tool_name,
                            input,
                            caller: None,
                        });
                    }
                }
                // Skip other item kinds (file_change, command_execution, etc.)
                _ => {}
            }
        }
        flush_assistant(&mut assistant_blocks, &mut messages);
        flush_user(&mut user_blocks, &mut messages);
    }

    messages
}

pub(crate) fn session_to_detail(session: SavedSession) -> SessionDetailResponse {
    let messages: Vec<serde_json::Value> = session
        .messages
        .iter()
        .map(|msg| {
            let content_blocks: Vec<serde_json::Value> = msg
                .content
                .iter()
                .map(|block| match block {
                    crate::models::ContentBlock::Text { text, .. } => {
                        serde_json::json!({ "type": "text", "text": text })
                    }
                    crate::models::ContentBlock::Thinking { thinking, .. } => {
                        serde_json::json!({ "type": "thinking", "text": thinking })
                    }
                    crate::models::ContentBlock::ToolUse { id, name, input, caller } => {
                        let mut obj =
                            serde_json::json!({ "type": "tool_use", "id": id, "name": name, "input": input });
                        if let Some(caller) = caller {
                            obj["caller"] = serde_json::json!(caller);
                        }
                        obj
                    }
                    crate::models::ContentBlock::ToolResult { tool_use_id, content, is_error, content_blocks, .. } => {
                        let mut obj = serde_json::json!({ "type": "tool_result", "tool_use_id": tool_use_id });
                        if let Some(cbs) = content_blocks {
                            obj["content_blocks"] = serde_json::json!(cbs);
                            if !content.is_empty() {
                                obj["content"] = serde_json::json!(content);
                            }
                        } else {
                            obj["content"] = serde_json::json!(content);
                        }
                        if let Some(e) = is_error {
                            obj["is_error"] = serde_json::json!(e);
                        }
                        obj
                    }
                    crate::models::ContentBlock::ServerToolUse { id, name, input } => {
                        serde_json::json!({ "type": "tool_use", "id": id, "name": name, "input": input })
                    }
                    crate::models::ContentBlock::ToolSearchToolResult { tool_use_id, content } => {
                        serde_json::json!({ "type": "tool_result", "tool_use_id": tool_use_id, "content": content })
                    }
                    crate::models::ContentBlock::CodeExecutionToolResult { tool_use_id, content } => {
                        serde_json::json!({ "type": "tool_result", "tool_use_id": tool_use_id, "content": content })
                    }
                    crate::models::ContentBlock::ImageUrl { .. } => serde_json::Value::Null,
                })
                .collect();
            serde_json::json!({
                "role": msg.role,
                "content": content_blocks,
            })
        })
        .collect();
    SessionDetailResponse {
        metadata: session.metadata,
        messages,
        system_prompt: session.system_prompt,
    }
}

pub(crate) fn total_tokens_from_thread_detail(detail: &ThreadDetail) -> u64 {
    detail
        .turns
        .iter()
        .filter_map(|turn| turn.usage.as_ref())
        .map(|usage| u64::from(usage.input_tokens) + u64::from(usage.output_tokens))
        .sum()
}

pub(crate) fn session_title_override(
    requested: Option<&str>,
    thread_title: Option<&str>,
) -> Option<String> {
    requested
        .and_then(nonempty_title)
        .or_else(|| thread_title.and_then(nonempty_title))
}

fn nonempty_title(title: &str) -> Option<String> {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(super::types::truncate_text(trimmed, 50))
    }
}

fn map_session_err(id: &str, err: std::io::Error, action: &str) -> ApiError {
    match err.kind() {
        std::io::ErrorKind::NotFound => ApiError::not_found(format!("Session '{id}' not found")),
        std::io::ErrorKind::InvalidData => {
            ApiError::bad_request(format!("Failed to parse session '{id}': {err}"))
        }
        std::io::ErrorKind::InvalidInput => {
            ApiError::bad_request(format!("Invalid session id '{id}'"))
        }
        _ => ApiError::internal(format!("Failed to {action} session '{id}': {err}")),
    }
}
