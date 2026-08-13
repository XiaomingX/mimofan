//! Turn-boundary automatic memory capture (lightweight, no extra LLM call).
//!
//! Unlike the `remember` / `remember_vector` tools — which require the model
//! to *explicitly* decide to persist something — this module mines the
//! just-finished turn for high-value signals using cheap heuristics
//! (regex / keyword rules) and surfaces them for the engine to persist.
//!
//! Design goals:
//! - **Zero LLM latency**: runs entirely on the local turn transcript.
//! - **Zero network / I/O when memory is off**: callers only persist when the
//!   file-memory or vector-memory backend is enabled (see `turn_loop.rs`).
//! - **Low noise**: entries shorter than a threshold are dropped, and near-
//!   duplicate content is skipped so we don't spam the memory store.
//!
//! This is the lightweight counterpart to CodeBuddy's "auto memory" — a model
//! decides what to save there; here we *pre-extract candidates* so the model
//! doesn't have to remember to call `remember`, while still keeping the
//! decision cheap and deterministic.

use regex::Regex;

use crate::models::Message;
use mimofan_memory::MemoryCategory;

/// A candidate memory extracted from a turn, ready to persist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemorySignal {
    pub category: MemoryCategory,
    pub content: String,
}

impl MemorySignal {
    #[must_use]
    pub fn new(category: MemoryCategory, content: String) -> Self {
        Self { category, content }
    }
}

/// Entries shorter than this (chars) are treated as noise and dropped.
const MIN_SIGNAL_LEN: usize = 12;

/// Cap on extracted signals per turn, to bound memory growth.
const MAX_SIGNALS_PER_TURN: usize = 6;

/// Pull the plain text out of a message's content blocks.
fn message_text(message: &Message) -> String {
    let mut out = String::new();
    for block in &message.content {
        if let crate::models::ContentBlock::Text { text, .. } = block {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(text);
        }
    }
    out
}

/// Heuristics per user signal category. Each returns an optional captured
/// content string when the line looks like a durable preference / correction /
/// decision.
struct Rule<'a> {
    pattern: Regex,
    category: MemoryCategory,
    /// When true, the captured group (or whole match) is stored verbatim as a
    /// feedback-style durable note; otherwise we store a normalized decision.
    store_verbatim: bool,
    _marker: std::marker::PhantomData<&'a ()>,
}

fn compile(pattern: &str) -> Regex {
    // `unwrap` is safe: all patterns below are static and validated at author time.
    Regex::new(pattern).expect("turn_memory regex must compile")
}

/// Build the rule table once (called lazily via `once_cell`/function-local statics).
fn rules() -> Vec<Rule<'static>> {
    // Order matters: explicit "remember/记住" wins, then corrections, then decisions.
    vec![
        // Explicit user instruction to remember a preference / fact.
        Rule {
            pattern: compile(r"(?im)(?:请)?(?:记住|remember|记一下|记着|备忘)\s*[:：]?\s*(.+)"),
            category: MemoryCategory::User,
            store_verbatim: true,
            _marker: std::marker::PhantomData,
        },
        // Stable preference / "always / 以后 / 偏好".
        Rule {
            pattern: compile(
                r"(?im)(?:我(?:的)?偏好|我习惯|以后|总是|一律|always|默认都|统一)\s*[:：]?\s*(.+)",
            ),
            category: MemoryCategory::Feedback,
            store_verbatim: true,
            _marker: std::marker::PhantomData,
        },
        // Correction of the agent's behavior.
        Rule {
            pattern: compile(r"(?im)(?:不对|错了|不要|别|不应该|应该|请改|纠正)\s*[:：]?\s*(.+)"),
            category: MemoryCategory::Feedback,
            store_verbatim: true,
            _marker: std::marker::PhantomData,
        },
        // Explicit decision / choice between options.
        Rule {
            pattern: compile(
                r"(?im)(?:决定(?:用|采用|使用)?|采用|选定|选用|确定用)\s*[:：]?\s*(.+)",
            ),
            category: MemoryCategory::Project,
            store_verbatim: true,
            _marker: std::marker::PhantomData,
        },
    ]
}

/// True when two candidate contents are near-duplicates (cheap check: identical
/// after collapsing whitespace + lowercasing, or one contains the other).
fn is_duplicate(a: &str, b: &str) -> bool {
    let a = a.trim().to_lowercase();
    let b = b.trim().to_lowercase();
    if a == b {
        return true;
    }
    // Ignore very short strings for the "contains" check to avoid false merges.
    if a.len() >= 8 && b.len() >= 8 {
        return a.contains(&b) || b.contains(&a);
    }
    false
}

/// Extract memory signals from a finished turn's transcript.
///
/// Only **user** messages are mined (agent self-talk is not a durable user
/// preference). The result is de-duplicated and length-filtered. Callers are
/// responsible for persisting (file / vector memory) and for honoring the
/// `memory.enabled` / `MIMOFAN_MEMORY_API_KEY` gates.
#[must_use]
pub fn extract_signals(messages: &[Message]) -> Vec<MemorySignal> {
    let rules = rules();
    let mut signals: Vec<MemorySignal> = Vec::new();

    for message in messages {
        if message.role != "user" {
            continue;
        }
        let text = message_text(message);
        if text.trim().is_empty() {
            continue;
        }

        for line in text.lines() {
            let line = line.trim();
            if line.chars().count() < MIN_SIGNAL_LEN {
                continue;
            }
            for rule in &rules {
                if let Some(captured) = rule.pattern.captures(line) {
                    let raw = captured
                        .get(1)
                        .map_or(line, |m| m.as_str())
                        .trim()
                        .to_string();
                    if raw.chars().count() < MIN_SIGNAL_LEN {
                        continue;
                    }
                    // De-duplicate against already-collected signals.
                    let dup = signals
                        .iter()
                        .any(|s| s.category == rule.category && is_duplicate(&s.content, &raw));
                    if !dup {
                        signals.push(MemorySignal::new(rule.category, raw));
                    }
                    break; // one rule per line is enough
                }
            }
            if signals.len() >= MAX_SIGNALS_PER_TURN {
                return signals;
            }
        }
    }
    signals
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
    fn extracts_explicit_remember() {
        let msgs = vec![user_msg(
            "请记住：我喜欢用 Rust 写工具，不要生成 Python 脚本",
        )];
        let signals = extract_signals(&msgs);
        assert!(signals.iter().any(|s| s.category == MemoryCategory::User));
        assert!(
            signals
                .iter()
                .any(|s| s.content.contains("Rust") && s.content.contains("Python")),
            "captured: {signals:?}"
        );
    }

    #[test]
    fn extracts_decision() {
        let msgs = vec![user_msg("决定采用 SQLite 而不是 Postgres 作为本地存储")];
        let signals = extract_signals(&msgs);
        assert!(signals.iter().any(|s| s.category == MemoryCategory::Project
            && s.content.contains("SQLite")
            && s.content.contains("Postgres")));
    }

    #[test]
    fn extracts_correction_as_feedback() {
        let msgs = vec![user_msg("不对，应该用异步调用，不要阻塞主线程")];
        let signals = extract_signals(&msgs);
        assert!(
            signals
                .iter()
                .any(|s| s.category == MemoryCategory::Feedback && s.content.contains("异步"))
        );
    }

    #[test]
    fn ignores_short_noise() {
        let msgs = vec![user_msg("ok")];
        assert!(extract_signals(&msgs).is_empty());
    }

    #[test]
    fn dedupes_same_line() {
        let msgs = vec![
            user_msg("请记住：这个项目默认使用中文进行所有沟通和回复"),
            user_msg("请记住：这个项目默认使用中文进行所有沟通和回复"),
        ];
        let signals = extract_signals(&msgs);
        let count = signals
            .iter()
            .filter(|s| s.content.contains("中文"))
            .count();
        assert_eq!(count, 1, "should dedupe identical captures");
    }

    #[test]
    fn skips_agent_messages() {
        let mut msgs = vec![user_msg("请记住：构建项目时统一使用 Cargo 而不是 make")];
        msgs.push(Message {
            role: "assistant".to_string(),
            content: vec![ContentBlock::Text {
                text: "请记住：构建项目时统一使用 Cargo 而不是 make".to_string(),
                cache_control: None,
            }],
        });
        // Assistant echo must not double-count beyond the user capture.
        let signals = extract_signals(&msgs);
        assert_eq!(signals.len(), 1);
    }
}
