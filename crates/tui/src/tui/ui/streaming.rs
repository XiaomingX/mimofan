//! streaming 子系统（从 ui 上帝文件切片）
use super::*;

/// Strip ANSI control codes / non-printable bytes from a streaming
/// text chunk. `pub(super)` because `tui::notifications` consumes it
/// from `super::ui` for its per-turn message composition.
pub(crate) fn sanitize_stream_chunk(chunk: &str) -> String {
    // Keep printable characters and common whitespace; drop control bytes.
    chunk
        .chars()
        .filter(|c| *c == '\n' || *c == '\t' || !c.is_control())
        .collect()
}

// Per-turn notification composition (settings, message body, summary)
// moved to `tui/notifications.rs` alongside the dispatch primitives.

/// Ensure an in-flight streaming Assistant cell exists in history and return
/// its index. Thinking cells go through `streaming_thinking::ensure_active_entry`
/// (active cell) instead.
pub(crate) fn ensure_streaming_assistant_history_cell(app: &mut App) -> usize {
    if let Some(index) = app.streaming_message_index {
        return index;
    }
    app.add_message(HistoryCell::Assistant {
        content: String::new(),
        streaming: true,
    });
    let index = app.history.len().saturating_sub(1);
    app.streaming_message_index = Some(index);
    index
}

pub(crate) fn append_streaming_text(app: &mut App, index: usize, text: &str) {
    if text.is_empty() {
        return;
    }
    if let Some(HistoryCell::Assistant { content, .. }) = app.history.get_mut(index) {
        content.push_str(text);
        // Bump only the streaming cell's per-cell revision so the transcript
        // cache re-renders just this cell. Without this, the cache would
        // either skip the update entirely (now that the global
        // history_version is no longer fanned out across every cell) or fall
        // back to a full re-wrap of the entire transcript every chunk.
        app.bump_history_cell(index);
    }
}

pub(crate) fn push_assistant_message(
    app: &mut App,
    text: String,
    thinking: Option<String>,
    tool_uses: PendingToolUses,
) {
    let mut blocks = Vec::new();
    if let Some(thinking) = thinking {
        blocks.push(ContentBlock::Thinking {
            thinking,
            signature: None,
        });
    }
    if !text.is_empty() {
        blocks.push(ContentBlock::Text {
            text,
            cache_control: None,
        });
    }
    for (id, name, input) in tool_uses {
        blocks.push(ContentBlock::ToolUse {
            id,
            name,
            input,
            caller: None,
        });
    }

    let has_sendable_content = blocks.iter().any(|block| {
        matches!(
            block,
            ContentBlock::Text { .. } | ContentBlock::ToolUse { .. }
        )
    });
    if has_sendable_content {
        app.api_messages.push(Message {
            role: "assistant".to_string(),
            content: blocks,
        });
    }
}

pub(crate) async fn tool_result_content_for_api_message(
    app: &App,
    id: &str,
    name: &str,
    output: &ToolResult,
) -> String {
    let raw = output.content.trim();
    if raw.is_empty() {
        return String::new();
    }

    if matches!(name, "run_tests" | "run_verifiers" | "task_gate_run") {
        return crate::core::engine::compact_tool_result_for_context(&app.model, name, output);
    }

    if raw.chars().count() > crate::tool_output_receipts::RAW_TOOL_OUTPUT_RECEIPT_THRESHOLD_CHARS {
        let messages = live_tool_receipt_messages(app, id, raw, output.success);
        let artifacts = app.session_artifacts.clone();
        let raw = raw.to_string();
        match tokio::task::spawn_blocking(move || {
            compact_live_tool_receipt(messages, artifacts, raw)
        })
        .await
        {
            Ok(Some(receipt)) => return receipt,
            Ok(None) => {}
            Err(err) => {
                crate::logging::warn(format!("live tool-output receipt compaction failed: {err}"));
            }
        }
    }

    crate::core::engine::compact_tool_result_for_context(&app.model, name, output)
}

fn live_tool_receipt_messages(app: &App, id: &str, raw: &str, success: bool) -> Vec<Message> {
    let mut messages = Vec::with_capacity(2);
    if let Some(tool_use_msg) = app.api_messages.iter().rev().find(|message| {
        message.content.iter().any(|block| {
            matches!(block, ContentBlock::ToolUse { id: tool_use_id, .. } if tool_use_id == id)
        })
    }) {
        messages.push(tool_use_msg.clone());
    }
    messages.push(Message {
        role: "user".to_string(),
        content: vec![ContentBlock::ToolResult {
            tool_use_id: id.to_string(),
            content: raw.to_string(),
            is_error: Some(!success),
            content_blocks: None,
        }],
    });
    messages
}

fn compact_live_tool_receipt(
    messages: Vec<Message>,
    artifacts: Vec<crate::artifacts::ArtifactRecord>,
    raw: String,
) -> Option<String> {
    let (compacted, _) =
        crate::tool_output_receipts::compact_messages_for_persistence(&messages, &artifacts);
    let content = compacted
        .last()
        .and_then(|message| message.content.first())
        .and_then(|block| match block {
            ContentBlock::ToolResult { content, .. } => Some(content),
            _ => None,
        })?;
    if content != &raw && live_tool_content_is_receipt(content) {
        Some(content.clone())
    } else {
        None
    }
}

fn live_tool_content_is_receipt(content: &str) -> bool {
    content.trim_start().starts_with("[TOOL_OUTPUT_RECEIPT]")
}

pub(crate) fn replace_matching_assistant_text(
    app: &mut App,
    original_text: &str,
    translated_text: String,
) -> bool {
    for message in app.api_messages.iter_mut().rev() {
        if message.role != "assistant" {
            continue;
        }
        for block in &mut message.content {
            if let ContentBlock::Text { text, .. } = block
                && text == original_text
            {
                *text = translated_text;
                return true;
            }
        }
    }
    false
}

// Streaming-thinking lifecycle helpers moved to `tui/streaming_thinking.rs`.
