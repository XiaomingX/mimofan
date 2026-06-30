//! Compact receipts for oversized tool outputs in saved session history.

use std::collections::HashMap;

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::artifacts::{ArtifactKind, ArtifactRecord, format_artifact_relative_path};
use crate::models::{ContentBlock, Message};
use crate::tools::truncate;

/// Match the provider-wire budget so persisted/resumed history does not keep a
/// larger raw body than the model would receive on a fresh request.
pub const RAW_TOOL_OUTPUT_RECEIPT_THRESHOLD_CHARS: usize = 12_000;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolOutputReceiptStats {
    pub compacted_count: usize,
    pub artifact_receipts: usize,
    pub sha_receipts: usize,
    pub unavailable_receipts: usize,
    pub original_chars: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolOutputStatus {
    pub raw_large_count: usize,
    pub raw_large_chars: usize,
    pub receipt_count: usize,
    pub artifact_count: usize,
    pub artifact_bytes: u64,
}

#[derive(Debug, Clone)]
struct ToolUseInfo {
    name: String,
    input: Value,
}

#[derive(Debug, Clone)]
enum DetailHandle {
    Artifact(ArtifactRecord),
    Sha { sha: String, persisted: bool },
}

/// Return a copy of `messages` with oversized raw tool-result bodies replaced
/// by compact receipts. Full output is kept behind existing session artifacts
/// when available; otherwise a SHA-addressed spillover copy is written for
/// `retrieve_tool_result`.
pub fn compact_messages_for_persistence(
    messages: &[Message],
    artifacts: &[ArtifactRecord],
) -> (Vec<Message>, ToolOutputReceiptStats) {
    let artifacts_by_call = artifacts_by_tool_call(artifacts);
    let mut tool_uses: HashMap<String, ToolUseInfo> = HashMap::new();
    let mut stats = ToolOutputReceiptStats::default();
    let mut compacted = Vec::with_capacity(messages.len());

    for message in messages {
        let mut next = message.clone();
        for block in &mut next.content {
            match block {
                ContentBlock::ToolUse {
                    id, name, input, ..
                } => {
                    tool_uses.insert(
                        id.clone(),
                        ToolUseInfo {
                            name: name.clone(),
                            input: input.clone(),
                        },
                    );
                }
                ContentBlock::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                    ..
                } => {
                    let char_count = content.chars().count();
                    if char_count <= RAW_TOOL_OUTPUT_RECEIPT_THRESHOLD_CHARS
                        || looks_like_receipt(content)
                    {
                        continue;
                    }

                    let tool_info = tool_uses.get(tool_use_id);
                    let handle = artifacts_by_call
                        .get(tool_use_id.as_str())
                        .cloned()
                        .map(|artifact| DetailHandle::Artifact((*artifact).clone()))
                        .unwrap_or_else(|| DetailHandle::Sha {
                            sha: sha256_hex(content.as_bytes()),
                            persisted: persist_sha_tool_result(content),
                        });
                    let source = match &handle {
                        DetailHandle::Artifact(_) => ReceiptSource::Artifact,
                        DetailHandle::Sha {
                            persisted: true, ..
                        } => ReceiptSource::Sha,
                        DetailHandle::Sha {
                            persisted: false, ..
                        } => ReceiptSource::Unavailable,
                    };

                    *content = render_tool_output_receipt(
                        tool_use_id,
                        tool_info,
                        content,
                        *is_error,
                        &handle,
                    );
                    stats.compacted_count += 1;
                    stats.original_chars = stats.original_chars.saturating_add(char_count);
                    match source {
                        ReceiptSource::Artifact => stats.artifact_receipts += 1,
                        ReceiptSource::Sha => stats.sha_receipts += 1,
                        ReceiptSource::Unavailable => stats.unavailable_receipts += 1,
                    }
                }
                _ => {}
            }
        }
        compacted.push(next);
    }

    (compacted, stats)
}

pub fn tool_output_status(messages: &[Message], artifacts: &[ArtifactRecord]) -> ToolOutputStatus {
    let mut status = ToolOutputStatus {
        artifact_count: artifacts.len(),
        artifact_bytes: artifacts
            .iter()
            .map(|artifact| artifact.byte_size)
            .sum::<u64>(),
        ..ToolOutputStatus::default()
    };

    for message in messages {
        for block in &message.content {
            if let ContentBlock::ToolResult { content, .. } = block {
                if looks_like_receipt(content) {
                    status.receipt_count += 1;
                } else {
                    let chars = content.chars().count();
                    if chars > RAW_TOOL_OUTPUT_RECEIPT_THRESHOLD_CHARS {
                        status.raw_large_count += 1;
                        status.raw_large_chars = status.raw_large_chars.saturating_add(chars);
                    }
                }
            }
        }
    }

    status
}

pub fn format_tool_output_status(status: &ToolOutputStatus) -> String {
    let mut parts = Vec::new();
    if status.raw_large_count > 0 {
        parts.push(format!(
            "{} raw over cap (~{} chars) adding context pressure",
            status.raw_large_count,
            format_count(status.raw_large_chars)
        ));
    }
    if status.receipt_count > 0 {
        parts.push(format!("{} compact receipt(s)", status.receipt_count));
    }
    if status.artifact_count > 0 {
        parts.push(format!(
            "{} artifact(s), {} stored",
            status.artifact_count,
            crate::artifacts::format_byte_size(status.artifact_bytes)
        ));
    }
    if parts.is_empty() {
        "no large outputs tracked".to_string()
    } else {
        parts.join("; ")
    }
}

fn artifacts_by_tool_call(artifacts: &[ArtifactRecord]) -> HashMap<&str, &ArtifactRecord> {
    artifacts
        .iter()
        .filter(|artifact| artifact.kind == ArtifactKind::ToolOutput)
        .map(|artifact| (artifact.tool_call_id.as_str(), artifact))
        .collect()
}

#[derive(Debug, Clone, Copy)]
enum ReceiptSource {
    Artifact,
    Sha,
    Unavailable,
}

fn render_tool_output_receipt(
    tool_call_id: &str,
    tool_info: Option<&ToolUseInfo>,
    original_content: &str,
    is_error: Option<bool>,
    handle: &DetailHandle,
) -> String {
    let original_chars = original_content.chars().count();
    let original_bytes = original_content.len() as u64;
    let tool_name = match handle {
        DetailHandle::Artifact(record) if !record.tool_name.trim().is_empty() => {
            record.tool_name.as_str()
        }
        _ => tool_info
            .map(|info| info.name.as_str())
            .filter(|name| !name.trim().is_empty())
            .unwrap_or("unknown"),
    };
    let command_or_query = tool_info
        .map(|info| summarize_input(&info.input, 300))
        .unwrap_or_else(|| "unknown".to_string());
    let status = if is_error.unwrap_or(false) {
        "error"
    } else {
        "success"
    };
    let exit_status = infer_exit_status(original_content).unwrap_or_else(|| "unknown".to_string());
    let preview = preview_for_receipt(handle, original_content);
    let (detail_handle, retrieve, storage) = match handle {
        DetailHandle::Artifact(record) => (
            record.id.clone(),
            format!("retrieve_tool_result ref={}", record.id),
            format_artifact_relative_path(&record.storage_path),
        ),
        DetailHandle::Sha { sha, persisted } => {
            let handle = format!("sha:{sha}");
            let storage = if *persisted {
                "content-addressed spillover".to_string()
            } else {
                "unavailable; spillover write failed".to_string()
            };
            (
                handle.clone(),
                format!("retrieve_tool_result ref={handle}"),
                storage,
            )
        }
    };

    format!(
        "[TOOL_OUTPUT_RECEIPT]\n\
         tool: {tool_name}\n\
         tool_call_id: {tool_call_id}\n\
         status: {status}\n\
         exit_status: {exit_status}\n\
         elapsed: unknown\n\
         output: {bytes} ({chars} chars, ~{tokens} tokens)\n\
         truncation: raw output omitted from saved/resumed context\n\
         detail_handle: {detail_handle}\n\
         retrieve: {retrieve}\n\
         storage: {storage}\n\
         command_or_query: {command_or_query}\n\
         preview: {preview}\n\
         [/TOOL_OUTPUT_RECEIPT]",
        bytes = crate::artifacts::format_byte_size(original_bytes),
        chars = format_count(original_chars),
        tokens = format_count(approx_tokens(original_chars)),
    )
}

fn persist_sha_tool_result(content: &str) -> bool {
    let sha = sha256_hex(content.as_bytes());
    match truncate::write_sha_spillover(&sha, content) {
        Ok(_) => true,
        Err(err) => {
            crate::logging::warn(format!(
                "tool-output receipt SHA spillover write failed for sha={sha}: {err}"
            ));
            false
        }
    }
}

fn preview_for_receipt(handle: &DetailHandle, original_content: &str) -> String {
    let preview = match handle {
        DetailHandle::Artifact(record) if !record.preview.trim().is_empty() => {
            record.preview.as_str()
        }
        _ => original_content,
    };
    summarize_text(preview, 240)
}

fn looks_like_receipt(content: &str) -> bool {
    let trimmed = content.trim_start();
    trimmed.starts_with("[TOOL_OUTPUT_RECEIPT]")
        || trimmed.starts_with("[artifact:")
        || trimmed.starts_with("[TOOL_RESULT_TRUNCATED]")
        || trimmed.starts_with("<TOOL_RESULT_REF")
}

fn infer_exit_status(content: &str) -> Option<String> {
    if let Ok(value) = serde_json::from_str::<Value>(content) {
        for key in ["exit_code", "exit_status", "status", "code"] {
            if let Some(value) = value.get(key) {
                return Some(summarize_input(value, 120));
            }
        }
    }

    for line in content.lines().take(40) {
        let trimmed = line.trim();
        for prefix in ["Exit code:", "exit code:", "Exit status:", "exit status:"] {
            if let Some(value) = trimmed.strip_prefix(prefix) {
                return Some(summarize_text(value.trim(), 120));
            }
        }
    }
    None
}

fn summarize_input(value: &Value, max_chars: usize) -> String {
    let raw = value
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| value.to_string());
    summarize_text(&raw, max_chars)
}

fn summarize_text(text: &str, max_chars: usize) -> String {
    let escaped = text.replace('\n', "\\n");
    let mut summary: String = escaped.chars().take(max_chars).collect();
    if escaped.chars().count() > max_chars {
        summary.push_str("...");
    }
    summary
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn approx_tokens(chars: usize) -> usize {
    chars.div_ceil(4)
}

fn format_count(value: usize) -> String {
    value.to_string()
}

#[cfg(test)]
mod tests {}
