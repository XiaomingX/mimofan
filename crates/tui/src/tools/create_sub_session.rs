//! `create_sub_session` — spawn a sibling session the model can delegate to.
//!
//! Issue #697 item 1. `ThreadRequest::Create` already exists in the protocol
//! and is exercised by the JSON-RPC/HTTP layer (`app-server`), but no tool
//! could derive a sibling session. This tool forwards the request through the
//! `thread_request_tx` channel on [`RuntimeToolServices`] and (in `first-turn`
//! mode) sends an opening message, returning the spawned thread id.

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::oneshot;

use mimofan_protocol::{ThreadRequest, ThreadResponse};

use crate::tools::spec::{
    ApprovalRequirement, RuntimeToolServices, ToolCapability, ToolContext, ToolError, ToolResult,
    ToolSpec,
};

/// `create_sub_session` tool.
pub struct CreateSubSessionTool;

type ThreadTx = UnboundedSender<(
    ThreadRequest,
    oneshot::Sender<Result<ThreadResponse, String>>,
)>;

#[async_trait]
impl ToolSpec for CreateSubSessionTool {
    fn name(&self) -> &str {
        "create_sub_session"
    }

    fn description(&self) -> &str {
        "Spawn a sibling session (separate thread) for parallel or delegated work. \
         `mode: \"sent\"` creates the session and returns its id for later use. \
         `mode: \"first-turn\"` additionally sends `prompt` as the opening message and \
         returns the first response. The sibling runs independently of the current turn."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "mode": {
                    "type": "string",
                    "enum": ["sent", "first-turn"],
                    "description": "sent = create only; first-turn = create and run an opening prompt."
                },
                "prompt": {
                    "type": "string",
                    "description": "Opening message, required when mode is first-turn."
                },
                "metadata": {
                    "type": "object",
                    "description": "Optional metadata attached to the new thread."
                }
            },
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
        let mode = input
            .get("mode")
            .and_then(Value::as_str)
            .unwrap_or("sent")
            .to_string();
        let metadata = input.get("metadata").cloned().unwrap_or_else(|| json!({}));

        let tx = thread_tx(&context.runtime)?;

        // 1) create the sibling thread.
        let create_resp = dispatch(
            tx,
            ThreadRequest::Create {
                metadata: metadata.clone(),
            },
        )
        .await?;
        let thread_id = create_resp.thread_id.clone();

        if mode == "first-turn" {
            let prompt = required_string(&input, "prompt")?;
            let turn_resp = dispatch(
                tx,
                ThreadRequest::Message {
                    thread_id: thread_id.clone(),
                    input: prompt,
                },
            )
            .await?;
            return Ok(ToolResult::success(
                json!({
                    "status": "created_first_turn",
                    "thread_id": thread_id,
                    "first_turn": turn_resp,
                })
                .to_string(),
            ));
        }

        Ok(ToolResult::success(
            json!({
                "status": "created",
                "thread_id": thread_id,
                "response": create_resp,
            })
            .to_string(),
        ))
    }
}

fn thread_tx(runtime: &RuntimeToolServices) -> Result<&ThreadTx, ToolError> {
    runtime.thread_request_tx.as_ref().ok_or_else(|| {
        ToolError::not_available("sub-session spawning is not available in this context")
    })
}

async fn dispatch(tx: &ThreadTx, req: ThreadRequest) -> Result<ThreadResponse, ToolError> {
    let (reply_tx, reply_rx) = oneshot::channel();
    tx.send((req, reply_tx)).map_err(|err| {
        ToolError::execution_failed(format!("failed to dispatch thread request: {err}"))
    })?;
    let response = reply_rx.await.map_err(|err| {
        ToolError::execution_failed(format!("thread request channel closed: {err}"))
    })?;
    response.map_err(ToolError::execution_failed)
}

fn required_string(input: &Value, key: &str) -> Result<String, ToolError> {
    input
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| ToolError::missing_field(key))
}
