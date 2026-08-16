//! `record_artifact` — let the model explicitly persist a durable artifact.
//!
//! Large tool outputs are already spilled to `~/.mimofan/sessions/<id>/artifacts/`
//! automatically (see `crate::artifacts` and `tool_routing.rs`), but the model
//! had no way to *declare* an arbitrary piece of work worth keeping. This tool
//! fills that gap (#697 item 2): write caller-supplied content to the session
//! artifact store and append a metadata record to the session index through the
//! `session_artifacts_tx` channel on [`RuntimeToolServices`].

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::sync::mpsc::UnboundedSender;
use uuid::Uuid;

use crate::artifacts::{ArtifactKind, ArtifactRecord, write_session_artifact};
use crate::tools::spec::{
    ApprovalRequirement, RuntimeToolServices, ToolCapability, ToolContext, ToolError, ToolResult,
    ToolSpec,
};

/// `record_artifact` tool.
pub struct RecordArtifactTool;

#[async_trait]
impl ToolSpec for RecordArtifactTool {
    fn name(&self) -> &str {
        "record_artifact"
    }

    fn description(&self) -> &str {
        "Persist a piece of work as a durable session artifact so it can be retrieved \
         later with `retrieve_tool_result`. Use it for code you generated, a diff, a \
         report, or any output the user should be able to reopen. The content is written \
         under the session artifact store and indexed by id."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "content": {
                    "type": "string",
                    "description": "The artifact body to persist."
                },
                "artifact_id": {
                    "type": "string",
                    "description": "Stable id used for later retrieval. Auto-generated when omitted."
                },
                "tool_name": {
                    "type": "string",
                    "description": "Label identifying what produced this artifact (defaults to 'record_artifact')."
                },
                "preview": {
                    "type": "string",
                    "description": "Short summary shown in listings. Defaults to the first 200 chars of content."
                }
            },
            "required": ["content"],
            "additionalProperties": false
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::WritesFiles]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    async fn execute(&self, input: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        let content = required_string(&input, "content")?;
        let session_id = context.runtime.active_thread_id.clone().ok_or_else(|| {
            ToolError::not_available("record_artifact requires an active session/thread id")
        })?;

        let artifact_id = match input.get("artifact_id").and_then(Value::as_str) {
            Some(value) if !value.trim().is_empty() => value.trim().to_string(),
            _ => format!("art_{}", &Uuid::new_v4().to_string()[..12]),
        };
        let tool_name = input
            .get("tool_name")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("record_artifact")
            .to_string();
        let preview = input
            .get("preview")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| content.chars().take(200).collect::<String>());

        let (absolute_path, _relative) =
            write_session_artifact(&session_id, &artifact_id, &content).map_err(|err| {
                ToolError::execution_failed(format!(
                    "failed to write artifact '{artifact_id}': {err}"
                ))
            })?;

        let record = ArtifactRecord {
            id: crate::artifacts::artifact_id_for_tool_call(&artifact_id),
            kind: ArtifactKind::ToolOutput,
            session_id: session_id.clone(),
            tool_call_id: artifact_id.clone(),
            tool_name,
            created_at: chrono::Utc::now(),
            byte_size: content.len() as u64,
            preview: preview.chars().take(200).collect(),
            storage_path: absolute_path.clone(),
        };

        send_artifact_record(&context.runtime, record.clone())?;

        Ok(ToolResult::success(
            json!({
                "status": "recorded",
                "id": record.id,
                "tool_call_id": record.tool_call_id,
                "session_id": session_id,
                "path": absolute_path.display().to_string(),
                "byte_size": record.byte_size,
                "retrieve": format!("retrieve_tool_result ref={}", record.id),
            })
            .to_string(),
        ))
    }
}

fn send_artifact_record(
    runtime: &RuntimeToolServices,
    record: ArtifactRecord,
) -> Result<(), ToolError> {
    let Some(tx): Option<&UnboundedSender<ArtifactRecord>> = runtime.session_artifacts_tx.as_ref()
    else {
        return Err(ToolError::not_available(
            "session artifact index is not available in this context",
        ));
    };
    tx.send(record)
        .map_err(|err| ToolError::execution_failed(format!("failed to index artifact: {err}")))?;
    Ok(())
}

fn required_string(input: &Value, key: &str) -> Result<String, ToolError> {
    input
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| ToolError::missing_field(key))
}
