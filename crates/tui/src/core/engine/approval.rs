//! Approval + user-input handshake for the agent loop.
//!
//! Extracted from `core/engine.rs` (P1.3). The agent loop blocks on these
//! two futures whenever a tool requires explicit approval (`await_tool_approval`)
//! or whenever a tool requests live user input (`await_user_input`). Channels
//! and engine state stay private to the parent module.

use std::time::Duration;

use crate::core::events::Event;
use crate::tools::plan::exit_plan_mode_plan_text;
use crate::tools::spec::{ToolError, ToolResult};
use crate::tools::user_input::{
    UserInputOption, UserInputQuestion, UserInputRequest, UserInputResponse,
};
use crate::tui::app::AppMode;

const USER_INPUT_TIMEOUT: Duration = Duration::from_secs(300);

use super::Engine;

#[derive(Debug, Clone)]
pub(super) enum ApprovalDecision {
    Approved {
        id: String,
    },
    Denied {
        id: String,
    },
    /// Retry a tool with an elevated sandbox policy.
    RetryWithPolicy {
        id: String,
        policy: crate::sandbox::SandboxPolicy,
    },
}

#[derive(Debug, Clone)]
pub(super) enum UserInputDecision {
    Submitted {
        id: String,
        response: UserInputResponse,
    },
    Cancelled {
        id: String,
    },
}

/// Result of awaiting tool approval from the user.
#[derive(Debug)]
pub(super) enum ApprovalResult {
    /// User approved the tool execution.
    Approved,
    /// User denied the tool execution.
    Denied,
    /// User requested retry with an elevated sandbox policy.
    RetryWithPolicy(crate::sandbox::SandboxPolicy),
}

impl Engine {
    /// Format a cancellation suffix when the engine knows the cause.
    /// Some internal cancellation paths still use the raw token while
    /// #1541 is open; those keep the legacy message without a guessed
    /// reason.
    fn cancel_reason_suffix(&self) -> String {
        let reason = match self.cancel_reason.lock() {
            Ok(slot) => *slot,
            Err(poisoned) => *poisoned.into_inner(),
        };
        match reason {
            Some(reason) => format!(" (reason: {})", reason.describe()),
            None => String::new(),
        }
    }

    pub(super) async fn await_tool_approval(
        &mut self,
        tool_id: &str,
    ) -> Result<ApprovalResult, ToolError> {
        loop {
            tokio::select! {
                _ = self.cancel_token.cancelled() => {
                    let suffix = self.cancel_reason_suffix();
                    return Err(ToolError::execution_failed(
                        format!("Request cancelled while awaiting approval{suffix}"),
                    ));
                }
                decision = self.rx_approval.recv() => {
                    let Some(decision) = decision else {
                        return Err(ToolError::execution_failed(
                            "Approval channel closed — engine is shutting down. \
                             The approval modal can no longer reach the engine; \
                             this is typically a teardown race, not a user action."
                                .to_string(),
                        ));
                    };
                    match decision {
                        ApprovalDecision::Approved { id } if id == tool_id => {
                            return Ok(ApprovalResult::Approved);
                        }
                        ApprovalDecision::Denied { id } if id == tool_id => {
                            return Ok(ApprovalResult::Denied);
                        }
                        ApprovalDecision::RetryWithPolicy { id, policy } if id == tool_id => {
                            return Ok(ApprovalResult::RetryWithPolicy(policy));
                        }
                        _ => continue,
                    }
                }
            }
        }
    }

    /// Present a finished plan to the user and block until they approve or
    /// reject it.
    ///
    /// On approval the plan is recorded on the shared `PlanState` and a
    /// [`Event::PlanModeApproved`] is emitted so the UI can drop out of Plan
    /// mode. On rejection the session stays in Plan mode and the model is told
    /// to revise — this is a normal outcome, not an error, so the turn can
    /// continue.
    pub(super) async fn await_plan_approval(
        &mut self,
        tool_id: &str,
        tool_input: &serde_json::Value,
        mode: AppMode,
    ) -> Result<ToolResult, ToolError> {
        if mode != AppMode::Plan {
            return Err(ToolError::invalid_input(
                "exit_plan_mode is only available in Plan mode",
            ));
        }

        let plan = exit_plan_mode_plan_text(tool_input)?;

        let request = UserInputRequest {
            questions: vec![UserInputQuestion {
                header: "计划审批".to_string(),
                id: "exit_plan_mode".to_string(),
                question: "计划已就绪，是否批准并开始实施？".to_string(),
                options: vec![
                    UserInputOption {
                        label: "批准并开始实施".to_string(),
                        description: "退出 Plan 模式，按此计划开始修改代码".to_string(),
                    },
                    UserInputOption {
                        label: "继续完善计划".to_string(),
                        description: "保持 Plan 模式，根据反馈修订计划".to_string(),
                    },
                ],
                allow_free_text: true,
                multi_select: false,
            }],
        };

        let response = self.await_user_input(tool_id, request).await?;
        let answer = response
            .answers
            .first()
            .ok_or_else(|| ToolError::execution_failed("Plan approval returned no answer"))?;

        // The modal's free-text escape hatch means the label is not guaranteed
        // to be one of the two options; treat only an explicit approval as
        // approval and route anything else back into plan revision, carrying
        // the user's wording so the model can act on it.
        if answer.label == "批准并开始实施" {
            {
                let mut state = self.config.plan_state.lock().await;
                state.set_approved_plan(plan.clone());
            }
            let _ = self.tx_event.send(Event::PlanModeApproved { plan }).await;
            Ok(ToolResult::success(
                "用户已批准计划，已退出 Plan 模式，可以开始实施。".to_string(),
            ))
        } else {
            let feedback = answer.value.trim();
            let detail = if feedback.is_empty() || feedback == answer.label {
                String::new()
            } else {
                format!("用户反馈：{feedback}")
            };
            Ok(ToolResult::success(format!(
                "用户未批准计划，仍处于 Plan 模式。请根据反馈修订计划后再次请求批准。{detail}"
            )))
        }
    }

    pub(super) async fn await_user_input(
        &mut self,
        tool_id: &str,
        request: UserInputRequest,
    ) -> Result<UserInputResponse, ToolError> {
        let _ = self
            .tx_event
            .send(Event::UserInputRequired {
                id: tool_id.to_string(),
                request,
            })
            .await;

        loop {
            tokio::select! {
                _ = self.cancel_token.cancelled() => {
                    let suffix = self.cancel_reason_suffix();
                    return Err(ToolError::execution_failed(
                        format!("Request cancelled while awaiting user input{suffix}"),
                    ));
                }
                result = tokio::time::timeout(USER_INPUT_TIMEOUT, self.rx_user_input.recv()) => {
                    match result {
                        Ok(Some(decision)) => {
                            match decision {
                                UserInputDecision::Submitted { id, response } if id == tool_id => {
                                    return Ok(response);
                                }
                                UserInputDecision::Cancelled { id } if id == tool_id => {
                                    return Err(ToolError::execution_failed(
                                        "User input cancelled".to_string(),
                                    ));
                                }
                                _ => continue,
                            }
                        }
                        Ok(None) => {
                            return Err(ToolError::execution_failed(
                                "User input channel closed".to_string(),
                            ));
                        }
                        Err(_) => {
                            let _ = self
                                .tx_event
                                .send(Event::Status {
                                    message: format!(
                                        "User input timed out after {}s",
                                        USER_INPUT_TIMEOUT.as_secs()
                                    ),
                                })
                                .await;
                            return Err(ToolError::execution_failed(
                                format!(
                                    "User input timed out after {}s",
                                    USER_INPUT_TIMEOUT.as_secs()
                                ),
                            ));
                        }
                    }
                }
            }
        }
    }
}
