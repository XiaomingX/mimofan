//! Crash / interruption recovery for the engine.
//!
//! When the TUI process dies mid-turn (panic, `kill -9`, power loss, OOM),
//! the durable session transcript on disk may end on an *unfinished* turn:
//! the last message is a user prompt that never received a completed
//! assistant reply, or an assistant message that issued `tool_use` blocks but
//! the corresponding `tool_result` replies were never written back. On the
//! next startup we want to detect that dangling state and resume/replay it so
//! the user does not silently lose the in-flight request.
//!
//! This module owns that detection. [`resume_interrupted_turn`] inspects the
//! loaded [`Session`] (read-only) and, when it finds a turn that was
//! interrupted before completion, returns the user-facing prompt text that
//! should be re-dispatched. The engine wires the returned prompt into a
//! `Op::SendMessage` so the model continues the turn from where it left off.
//!
//! Detection rules (the transcript is append-only and ordered oldest→newest):
//!
//! 1. **Dangling user prompt** — the last message has role `user` and there is
//!    no subsequent `assistant` message. The user asked something, the model
//!    never answered. Replay the user text verbatim.
//! 2. **Dangling tool calls** — the last message is an `assistant` message that
//!    contains one or more `tool_use` blocks, but the transcript has no
//!    following `tool_result` for at least one of those tool-use ids. The
//!    assistant's tool requests were never fulfilled (the crash happened while
//!    tools ran). Replay the originating user prompt so the whole turn is
//!    recomputed rather than half-applied.

use crate::core::session::Session;
use crate::models::ContentBlock;

/// The text of an interrupted turn that should be resumed on startup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumableTurn {
    /// The user prompt that kicked off the (unfinished) turn.
    pub prompt: String,
    /// Why recovery is kicking in — surfaced in logs/telemetry.
    pub reason: ResumeReason,
}

/// Why [`resume_interrupted_turn`] decided a turn was interrupted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResumeReason {
    /// Last transcript message was a `user` message with no assistant reply.
    DanglingUserPrompt,
    /// Last transcript message was an `assistant` message with unfulfilled
    /// `tool_use` calls (their `tool_result` replies are missing).
    UnfulfilledToolCalls,
}

impl std::fmt::Display for ResumeReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            ResumeReason::DanglingUserPrompt => "dangling_user_prompt",
            ResumeReason::UnfulfilledToolCalls => "unfulfilled_tool_calls",
        };
        f.write_str(s)
    }
}

/// Inspect a loaded [`Session`] for an interrupted turn.
///
/// Returns `Some(ResumableTurn)` when the durable transcript ends on an
/// unfinished turn, or `None` when the transcript is well-formed (every user
/// prompt has a completed assistant reply, and every tool call has its
/// result). The function is purely observational — it never mutates `session`.
///
/// A brand-new or already-compacted session (no messages, or ending on an
/// `assistant` message with no pending tool calls) is reported as `None`.
#[must_use]
pub fn resume_interrupted_turn(session: &Session) -> Option<ResumableTurn> {
    let messages = &session.messages;
    let last = messages.last()?;

    match last.role.as_str() {
        // Rule 1: the most recent message is a bare user prompt — the model
        // never produced a reply for it.
        "user" => Some(ResumableTurn {
            prompt: extract_text(last),
            reason: ResumeReason::DanglingUserPrompt,
        }),
        // Rule 2: the most recent message is an assistant message. Only treat
        // it as interrupted when it actually made tool calls that were never
        // answered. A normal completed assistant text reply ends the turn
        // cleanly and must NOT be resumed.
        "assistant" => {
            let tool_use_ids = collect_tool_use_ids(last);
            if tool_use_ids.is_empty() {
                return None;
            }
            let answered = collect_tool_result_ids(messages);
            if tool_use_ids.iter().all(|id| answered.contains(id)) {
                // All tool calls were fulfilled; turn completed normally.
                return None;
            }
            // Find the user prompt that started this turn (walk backwards past
            // any assistant/tool_result noise to the nearest user message).
            let prompt = messages
                .iter()
                .rev()
                .find(|m| m.role == "user")
                .map(extract_text)
                .unwrap_or_default();
            Some(ResumableTurn {
                prompt,
                reason: ResumeReason::UnfulfilledToolCalls,
            })
        }
        // Any other terminal role (e.g. `system`, `tool`) is not a recoverable
        // dangling turn.
        _ => None,
    }
}

/// Recursively pull the plain-text content out of a message's content blocks.
fn extract_text(message: &crate::models::Message) -> String {
    let mut out = String::new();
    for block in &message.content {
        match block {
            ContentBlock::Text { text, .. } => {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(text);
            }
            // Tool results can carry text too; include them so a resumed
            // prompt is faithful to what the user actually sent.
            ContentBlock::ToolResult { content, .. } => {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(content);
            }
            _ => {}
        }
    }
    out
}

/// Collect the tool-use ids issued by an assistant message.
fn collect_tool_use_ids(message: &crate::models::Message) -> Vec<String> {
    message
        .content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::ToolUse { id, .. } => Some(id.clone()),
            _ => None,
        })
        .collect()
}

/// Collect every tool-use id that already has a `tool_result` reply somewhere
/// in the transcript.
fn collect_tool_result_ids(
    messages: &[crate::models::Message],
) -> std::collections::HashSet<String> {
    let mut answered = std::collections::HashSet::new();
    for message in messages {
        for block in &message.content {
            if let ContentBlock::ToolResult { tool_use_id, .. } = block {
                answered.insert(tool_use_id.clone());
            }
        }
    }
    answered
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Message;

    fn user(text: &str) -> Message {
        Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: text.to_string(),
                cache_control: None,
            }],
        }
    }

    fn assistant_text(text: &str) -> Message {
        Message {
            role: "assistant".to_string(),
            content: vec![ContentBlock::Text {
                text: text.to_string(),
                cache_control: None,
            }],
        }
    }

    fn assistant_tool_use(id: &str) -> Message {
        Message {
            role: "assistant".to_string(),
            content: vec![ContentBlock::ToolUse {
                id: id.to_string(),
                name: "read_file".to_string(),
                input: serde_json::json!({}),
                caller: None,
            }],
        }
    }

    fn tool_result(id: &str) -> Message {
        Message {
            role: "user".to_string(),
            content: vec![ContentBlock::ToolResult {
                tool_use_id: id.to_string(),
                content: "ok".to_string(),
                is_error: None,
                content_blocks: None,
            }],
        }
    }

    fn session_with(messages: Vec<Message>) -> Session {
        let mut s = Session::new(
            "x".to_string(),
            ".".into(),
            false,
            false,
            ".".into(),
            ".".into(),
        );
        for m in messages {
            s.messages.push(m);
        }
        s
    }

    #[test]
    fn empty_session_is_not_interrupted() {
        let s = Session::new(
            "x".to_string(),
            ".".into(),
            false,
            false,
            ".".into(),
            ".".into(),
        );
        assert!(resume_interrupted_turn(&s).is_none());
    }

    #[test]
    fn dangling_user_prompt_is_resumed() {
        let s = session_with(vec![user("fix the bug in main.rs")]);
        let resumed = resume_interrupted_turn(&s).expect("should detect dangling prompt");
        assert_eq!(resumed.reason, ResumeReason::DanglingUserPrompt);
        assert_eq!(resumed.prompt, "fix the bug in main.rs");
    }

    #[test]
    fn completed_turn_is_not_resumed() {
        let s = session_with(vec![user("hi"), assistant_text("hello")]);
        assert!(resume_interrupted_turn(&s).is_none());
    }

    #[test]
    fn fulfilled_tool_calls_are_not_resumed() {
        // assistant asks for tool, tool result follows -> turn complete.
        let s = session_with(vec![
            user("read it"),
            assistant_tool_use("t1"),
            tool_result("t1"),
            assistant_text("done"),
        ]);
        assert!(resume_interrupted_turn(&s).is_none());
    }

    #[test]
    fn unfulfilled_tool_calls_are_resumed_with_user_prompt() {
        // assistant asked for a tool, crash before the result was written.
        let s = session_with(vec![user("read main.rs"), assistant_tool_use("t1")]);
        let resumed = resume_interrupted_turn(&s).expect("should detect dangling tool call");
        assert_eq!(resumed.reason, ResumeReason::UnfulfilledToolCalls);
        assert_eq!(resumed.prompt, "read main.rs");
    }
}
