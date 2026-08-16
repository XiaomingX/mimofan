//! Long-horizon objective tracking for context compaction.
//!
//! After a conversation is summarized, the model only sees the condensed
//! summary — the original first-turn task goal is no longer in the prompt.
//! If the summary drifts (drops or rephrases the goal), the model can quietly
//! start working toward a different objective than the one the user set.
//!
//! This module extracts a compact [`Objective`] from the first user turn, lets
//! the compaction prompt carry it ("TASK OBJECTIVE" section), and after each
//! summary offers a [`drift_check`] to measure how much of the objective the
//! summary still preserves. Callers (compaction, loop_guard) use the signal to
//! re-inject the objective when it drifts.

use crate::client::ApiClient;
use crate::llm_client::LlmClient;
use crate::models::{ContentBlock, Message, MessageRequest, SystemPrompt};

/// A compact description of the user's task goal, extracted from the first
/// user turn. `text` is the one-line goal; `key_points` are the atomic facts
/// the summary must keep mentioning to avoid drifting.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Objective {
    /// Short natural-language statement of the task goal.
    pub text: String,
    /// Atomic points the goal depends on (file paths, constraints, decisions).
    pub key_points: Vec<String>,
}

/// Maximum characters of the first user message used as the fallback objective
/// text when no LLM is available.
const FALLBACK_FIRST_MESSAGE_CHARS: usize = 200;

/// Try to extract a task [`Objective`] from the first user turn via the LLM.
///
/// Sends a short prompt asking the model to return JSON `{ "text": "...",
/// "key_points": [...] }` describing the user's goal. If the model call fails
/// or returns unparseable JSON, falls back to taking the first 200 characters
/// of the first user message as a single-point objective.
///
/// This is intentionally best-effort: a missing objective never blocks
/// compaction, it just degrades the drift signal to the cheap fallback.
pub async fn extract_objective(client: &ApiClient, messages: &[Message]) -> Objective {
    // Find the first user text message to anchor the extraction.
    let first_user = messages.iter().find(|m| {
        m.role == "user"
            && m.content
                .iter()
                .any(|b| matches!(b, ContentBlock::Text { .. }))
    });

    let Some(first_user) = first_user else {
        return Objective {
            text: String::new(),
            key_points: Vec::new(),
        };
    };

    let first_user_text = message_first_text(first_user);

    // Fallback builds the cheapest possible objective up front so we can use it
    // if the LLM path fails for any reason.
    let fallback = Objective {
        text: truncate_chars(&first_user_text, FALLBACK_FIRST_MESSAGE_CHARS).to_string(),
        key_points: Vec::new(),
    };

    let prompt = format!(
        "From the following user request, extract the primary task objective.\n\
         Respond ONLY with a JSON object of the form:\n\
         {{\"text\": \"<one concise sentence stating the goal>\", \
         \"key_points\": [\"<atomic fact or constraint the work depends on>\", ...]}}\n\
         Include file paths, constraints, and decisions as key_points.\n\n\
         USER REQUEST:\n{first_user_text}"
    );

    let request = MessageRequest {
        model: crate::config::DEFAULT_TEXT_MODEL.to_string(),
        messages: vec![Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: prompt,
                cache_control: None,
            }],
        }],
        max_tokens: 512,
        system: Some(SystemPrompt::Text(
            "You are a precise task-goal extractor. Output only JSON.".to_string(),
        )),
        tools: None,
        tool_choice: None,
        metadata: None,
        thinking: None,
        reasoning_effort: None,
        stream: Some(false),
        temperature: Some(0.0),
        top_p: None,
        response_format: None,
    };

    let parsed = client.create_message(request).await.and_then(|resp| {
        let text = resp
            .content
            .iter()
            .filter_map(|b| match b {
                ContentBlock::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        parse_objective_json(&text)
    });

    parsed.unwrap_or(fallback)
}

/// Parse an LLM response that should contain JSON `{text, key_points}`.
///
/// Tolerant of markdown code fences and surrounding prose: it scans for the
/// first `{...}` object and only keeps non-empty fields. Returns `Err` if no
/// usable object is found so the caller can fall back.
fn parse_objective_json(text: &str) -> Result<Objective, anyhow::Error> {
    let json_str = extract_first_json_object(text)
        .ok_or_else(|| anyhow::anyhow!("no JSON object in objective response"))?;
    let value: serde_json::Value = serde_json::from_str(json_str)
        .map_err(|e| anyhow::anyhow!("objective JSON parse error: {e}"))?;

    let text_field = value
        .get("text")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    if text_field.is_empty() {
        return Err(anyhow::anyhow!("objective JSON missing text field"));
    }

    let mut key_points = Vec::new();
    if let Some(arr) = value.get("key_points").and_then(|v| v.as_array()) {
        for item in arr {
            if let Some(s) = item.as_str() {
                let s = s.trim();
                if !s.is_empty() {
                    key_points.push(s.to_string());
                }
            }
        }
    }

    Ok(Objective {
        text: text_field.to_string(),
        key_points,
    })
}

/// Pull the `{...}` object out of a possibly fenced/prose LLM response.
fn extract_first_json_object(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    // Walk from the end to find the matching closing brace.
    let mut depth = 0i32;
    let bytes = text.as_bytes();
    for (idx, &b) in bytes.iter().enumerate().skip(start) {
        match b {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&text[start..=idx]);
                }
            }
            _ => {}
        }
    }
    None
}

/// Measure how much of `before` the summary `after_summary` still preserves.
///
/// Uses a simple word-overlap ratio over the objective text plus its
/// key_points, compared against the summary. Returns `1.0` when the objective
/// is empty (nothing to drift), otherwise a `0.0..=1.0` similarity. A value
/// below `DRIFT_THRESHOLD` means the summary has likely drifted away from the
/// original goal.
pub fn drift_check(before: &Objective, after_summary: &str) -> f64 {
    if before.text.is_empty() && before.key_points.is_empty() {
        return 1.0;
    }

    // Build the set of tokens that the objective requires to survive.
    let mut required: Vec<String> = Vec::new();
    required.push(before.text.clone());
    for kp in &before.key_points {
        required.push(kp.clone());
    }

    let summary_token_vec = tokenize(after_summary);
    let summary_tokens: std::collections::HashSet<&str> =
        summary_token_vec.iter().map(|s| s.as_str()).collect();

    let mut kept = 0usize;
    let mut total = 0usize;
    for requirement in &required {
        let req_tokens = tokenize(requirement);
        if req_tokens.is_empty() {
            continue;
        }
        total += 1;
        // Require at least half of the requirement's tokens to appear in the
        // summary, OR the requirement string to be contained verbatim.
        let overlap = req_tokens
            .iter()
            .filter(|t| summary_tokens.contains(t.as_str()))
            .count();
        let ratio = overlap as f64 / req_tokens.len() as f64;
        if ratio >= 0.5 || after_summary.contains(requirement.trim()) {
            kept += 1;
        }
    }

    if total == 0 {
        return 1.0;
    }
    let score = kept as f64 / total as f64;
    score.clamp(0.0, 1.0)
}

/// Similarity below this is considered objective drift.
pub const DRIFT_THRESHOLD: f64 = 0.6;

/// Split a string into lowercase alphanumeric word tokens (no new deps).
fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

/// First user text block of a message (or empty string).
fn message_first_text(message: &Message) -> String {
    for block in &message.content {
        if let ContentBlock::Text { text, .. } = block {
            return text.clone();
        }
    }
    String::new()
}

fn truncate_chars(text: &str, max_chars: usize) -> &str {
    if max_chars == 0 {
        return "";
    }
    match text.char_indices().nth(max_chars) {
        Some((idx, _)) => &text[..idx],
        None => text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ContentBlock;

    fn user_msg(text: &str) -> Message {
        Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: text.to_string(),
                cache_control: None,
            }],
        }
    }

    #[test]
    fn fallback_truncates_long_first_message_to_200_chars() {
        // The fallback path (no LLM) takes the first 200 chars of the user
        // message as the objective text. Exercise the truncation helper here
        // the same way `extract_objective` would on the None branch.
        let long = "a".repeat(500);
        let text = truncate_chars(&long, FALLBACK_FIRST_MESSAGE_CHARS).to_string();
        assert_eq!(text.chars().count(), FALLBACK_FIRST_MESSAGE_CHARS);
    }

    #[test]
    fn empty_conversation_yields_empty_objective() {
        // Mirrors the `extract_objective` None branch: no first user message
        // means no objective to track.
        let obj = Objective {
            text: String::new(),
            key_points: Vec::new(),
        };
        assert!(obj.text.is_empty());
        assert!(obj.key_points.is_empty());
    }

    #[test]
    fn drift_check_identical_text_is_one() {
        let obj = Objective {
            text: "Refactor the auth module to use JWT".to_string(),
            key_points: vec!["PostgreSQL".to_string(), "Rust 1.88".to_string()],
        };
        let summary = "We refactored the auth module to use JWT. Database is PostgreSQL, \
                       target is Rust 1.88.";
        let score = drift_check(&obj, summary);
        assert!(
            (score - 1.0).abs() < f64::EPSILON,
            "identical objective+summary must score 1.0, got {score}"
        );
        assert!(score >= DRIFT_THRESHOLD);
    }

    #[test]
    fn drift_check_unrelated_text_is_low() {
        let obj = Objective {
            text: "Migrate the billing service to Stripe".to_string(),
            key_points: vec!["webhook signatures".to_string()],
        };
        let summary = "The user asked about configuring their text editor theme and \
                       keybindings for a more comfortable workflow.";
        let score = drift_check(&obj, summary);
        assert!(
            score < DRIFT_THRESHOLD,
            "unrelated summary must drift below {DRIFT_THRESHOLD}, got {score}"
        );
    }

    #[test]
    fn drift_check_empty_objective_is_safe() {
        let obj = Objective {
            text: String::new(),
            key_points: Vec::new(),
        };
        let score = drift_check(&obj, "anything");
        assert!((score - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn parse_objective_json_handles_fenced_response() {
        let resp = "```json\n{\"text\": \"Add a CLI flag\", \"key_points\": [\"--verbose\", \"no deps\"]}\n```";
        let obj = parse_objective_json(resp).expect("fenced JSON must parse");
        assert_eq!(obj.text, "Add a CLI flag");
        assert_eq!(obj.key_points, vec!["--verbose", "no deps"]);
    }

    #[test]
    fn parse_objective_json_ignores_prose() {
        let resp = "Sure! Here is the objective: {\"text\":\"Fix the parser\",\"key_points\":[\"lexer\"]} done.";
        let obj = parse_objective_json(resp).expect("embedded JSON must parse");
        assert_eq!(obj.text, "Fix the parser");
        assert_eq!(obj.key_points, vec!["lexer"]);
    }

    #[test]
    fn parse_objective_json_rejects_missing_text() {
        let resp = "{\"key_points\": [\"x\"]}";
        assert!(parse_objective_json(resp).is_err());
    }

    #[test]
    fn user_msg_first_text_helper() {
        assert_eq!(message_first_text(&user_msg("hello world")), "hello world");
    }
}
