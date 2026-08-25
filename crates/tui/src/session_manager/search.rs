//! Cross-session full-text search over saved conversation **bodies**.
//!
//! `SessionManager::search_sessions` only matches session titles. This module
//! adds body-level search: it scans each saved session's message content
//! (user/assistant text, thinking, tool inputs and tool results) and returns
//! the sessions that contain the query, together with a short highlighted
//! snippet around the first match.
//!
//! Design notes (why no external FTS engine):
//! - Saved sessions are individual `<id>.json` files capped at `MAX_SESSIONS`
//!   (50). A linear scan of a few dozen small JSON files is well within
//!   interactive latency, so we avoid pulling in tantivy/FTS and the index
//!   lifecycle (staleness, rebuild, corruption) it would bring.
//! - Matching is case-insensitive substring on extracted plain text. Query
//!   terms are AND-combined when whitespace-separated so multi-word queries
//!   narrow rather than widen (mirrors how users expect search to behave).

use super::{SavedSession, SessionManager, SessionMetadata};
use crate::models::{ContentBlock, Message};

/// One matching session with a snippet of the first body hit.
#[derive(Debug, Clone)]
pub struct SessionSearchHit {
    /// Metadata of the matching session (title, timestamps, model, …).
    pub metadata: SessionMetadata,
    /// Number of message blocks that contained at least one query term.
    pub match_count: usize,
    /// A short excerpt around the first match, with surrounding context.
    /// `None` when the match was in the title only.
    pub snippet: Option<String>,
    /// Role of the message the snippet came from (`user`/`assistant`/…),
    /// or `title` when the hit was a title-only match.
    pub matched_in: String,
}

impl From<SessionMetadata> for SessionSearchHit {
    fn from(metadata: SessionMetadata) -> Self {
        SessionSearchHit {
            metadata,
            match_count: 0,
            snippet: None,
            matched_in: "list".to_string(),
        }
    }
}

impl From<&SessionMetadata> for SessionSearchHit {
    fn from(metadata: &SessionMetadata) -> Self {
        SessionSearchHit {
            metadata: metadata.clone(),
            match_count: 0,
            snippet: None,
            matched_in: "list".to_string(),
        }
    }
}

/// Extract searchable plain text from a single content block.
///
/// Returns `None` for blocks that carry no user-meaningful text (images).
fn block_text(block: &ContentBlock) -> Option<String> {
    match block {
        ContentBlock::Text { text, .. } => Some(text.clone()),
        ContentBlock::Thinking { thinking, .. } => Some(thinking.clone()),
        ContentBlock::ToolUse { name, input, .. }
        | ContentBlock::ServerToolUse { name, input, .. } => Some(format!("{name} {input}")),
        ContentBlock::ToolResult { content, .. } => Some(content.clone()),
        ContentBlock::ToolSearchToolResult { content, .. }
        | ContentBlock::CodeExecutionToolResult { content, .. } => Some(content.to_string()),
        ContentBlock::ImageUrl { .. } => None,
    }
}

/// Concatenate all searchable text from a message into a single lowercase-able
/// string plus its role.
fn message_text(message: &Message) -> String {
    let mut out = String::new();
    for block in &message.content {
        if let Some(text) = block_text(block) {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&text);
        }
    }
    out
}

/// Split a query into lowercased, non-empty terms (AND-combined).
fn query_terms(query: &str) -> Vec<String> {
    query
        .split_whitespace()
        .map(|t| t.to_lowercase())
        .filter(|t| !t.is_empty())
        .collect()
}

/// Return true when `haystack_lower` contains **every** term.
fn contains_all_terms(haystack_lower: &str, terms: &[String]) -> bool {
    terms.iter().all(|t| haystack_lower.contains(t))
}

/// Build a `~max_chars` snippet centred on the first occurrence of any term,
/// clamped to char boundaries, with ellipses when truncated.
fn make_snippet(text: &str, terms: &[String], max_chars: usize) -> Option<String> {
    let lower = text.to_lowercase();
    // Byte offset of the earliest matching term.
    let first = terms.iter().filter_map(|t| lower.find(t.as_str())).min()?;

    // Convert byte offset to char index so we can window safely on chars.
    let char_idx = lower[..first].chars().count();
    let chars: Vec<char> = text.chars().collect();
    let half = max_chars / 2;
    let start = char_idx.saturating_sub(half);
    let end = (char_idx + half).min(chars.len());

    let mut snippet = String::new();
    if start > 0 {
        snippet.push('…');
    }
    for &c in &chars[start..end] {
        // Collapse newlines/tabs to spaces for a single-line snippet.
        snippet.push(if c == '\n' || c == '\r' || c == '\t' {
            ' '
        } else {
            c
        });
    }
    if end < chars.len() {
        snippet.push('…');
    }
    Some(snippet.trim().to_string())
}

/// Search a single loaded session for the given terms. Returns a hit when the
/// title or any message body contains all terms.
fn search_one(session: &SavedSession, terms: &[String]) -> Option<SessionSearchHit> {
    let title_lower = session.metadata.title.to_lowercase();
    let mut match_count = 0usize;
    let mut first_snippet: Option<(String, String)> = None; // (role, snippet)

    for message in &session.messages {
        let text = message_text(message);
        if text.is_empty() {
            continue;
        }
        let lower = text.to_lowercase();
        if contains_all_terms(&lower, terms) {
            match_count += 1;
            if first_snippet.is_none() {
                let snippet = make_snippet(&text, terms, 160);
                if let Some(snippet) = snippet {
                    first_snippet = Some((message.role.clone(), snippet));
                }
            }
        }
    }

    if let Some((role, snippet)) = first_snippet {
        return Some(SessionSearchHit {
            metadata: session.metadata.clone(),
            match_count,
            snippet: Some(snippet),
            matched_in: role,
        });
    }

    // Title-only fallback so full-text search is a strict superset of the
    // legacy title search.
    if contains_all_terms(&title_lower, terms) {
        return Some(SessionSearchHit {
            metadata: session.metadata.clone(),
            match_count: 0,
            snippet: None,
            matched_in: "title".to_string(),
        });
    }

    None
}

impl SessionManager {
    /// Full-text search across all saved session **bodies** (not just titles).
    ///
    /// Multi-word queries are AND-combined. Results are ordered by relevance
    /// (more matching blocks first), breaking ties by most-recently-updated.
    /// Empty/whitespace-only queries return an empty result set.
    pub fn search_sessions_fulltext(&self, query: &str) -> std::io::Result<Vec<SessionSearchHit>> {
        let terms = query_terms(query);
        if terms.is_empty() {
            return Ok(Vec::new());
        }

        let mut hits: Vec<SessionSearchHit> = Vec::new();
        // Iterate metadata (already sorted newest-first) and load each body.
        for meta in self.list_sessions()? {
            // Skip unreadable/newer-schema sessions rather than failing the
            // whole search — a single corrupt file must not blind the query.
            let Ok(session) = self.load_session(&meta.id) else {
                continue;
            };
            if let Some(hit) = search_one(&session, &terms) {
                hits.push(hit);
            }
        }

        // Relevance: body matches (higher match_count) before title-only hits;
        // ties resolved by recency, which is already the input order.
        hits.sort_by_key(|h| std::cmp::Reverse(h.match_count));
        Ok(hits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_block(s: &str) -> ContentBlock {
        ContentBlock::Text {
            text: s.to_string(),
            cache_control: None,
        }
    }

    fn msg(role: &str, text: &str) -> Message {
        Message {
            role: role.to_string(),
            content: vec![text_block(text)],
        }
    }

    #[test]
    fn query_terms_splits_and_lowercases() {
        assert_eq!(query_terms("  Foo   BAR "), vec!["foo", "bar"]);
        assert!(query_terms("   ").is_empty());
    }

    #[test]
    fn contains_all_terms_is_and_combined() {
        assert!(contains_all_terms(
            "the quick brown fox",
            &["quick".into(), "fox".into()]
        ));
        assert!(!contains_all_terms(
            "the quick brown fox",
            &["quick".into(), "cat".into()]
        ));
    }

    #[test]
    fn snippet_centres_on_match_and_marks_truncation() {
        let text = "a".repeat(200) + "NEEDLE" + &"b".repeat(200);
        let snippet = make_snippet(&text, &["needle".into()], 40).unwrap();
        assert!(snippet.contains("NEEDLE"));
        assert!(snippet.starts_with('…'));
        assert!(snippet.ends_with('…'));
    }

    #[test]
    fn snippet_handles_multibyte_without_panicking() {
        let text = "前置文本中文内容 KUBERNETES 后置中文内容更多字";
        let snippet = make_snippet(text, &["kubernetes".into()], 20).unwrap();
        assert!(snippet.to_lowercase().contains("kubernetes"));
    }

    #[test]
    fn search_one_matches_body_not_just_title() {
        let session = SavedSession {
            schema_version: 1,
            metadata: sample_metadata("unrelated title"),
            messages: vec![
                msg("user", "how do I configure PostgreSQL connection pooling?"),
                msg("assistant", "Use a bounded pool size."),
            ],
            system_prompt: None,
            context_references: Vec::new(),
            artifacts: Vec::new(),
        };
        let hit = search_one(&session, &["postgresql".into()]).expect("should match body");
        assert_eq!(hit.match_count, 1);
        assert_eq!(hit.matched_in, "user");
        assert!(hit.snippet.unwrap().to_lowercase().contains("postgresql"));
    }

    #[test]
    fn search_one_title_only_fallback() {
        let session = SavedSession {
            schema_version: 1,
            metadata: sample_metadata("Deploying to Kubernetes"),
            messages: vec![msg("user", "hello")],
            system_prompt: None,
            context_references: Vec::new(),
            artifacts: Vec::new(),
        };
        let hit = search_one(&session, &["kubernetes".into()]).expect("title match");
        assert_eq!(hit.match_count, 0);
        assert_eq!(hit.matched_in, "title");
        assert!(hit.snippet.is_none());
    }

    #[test]
    fn search_one_and_combined_requires_all_terms() {
        let session = SavedSession {
            schema_version: 1,
            metadata: sample_metadata("t"),
            messages: vec![msg("assistant", "we discussed docker and compose")],
            system_prompt: None,
            context_references: Vec::new(),
            artifacts: Vec::new(),
        };
        assert!(search_one(&session, &["docker".into(), "compose".into()]).is_some());
        assert!(search_one(&session, &["docker".into(), "kafka".into()]).is_none());
    }

    #[test]
    fn tool_blocks_are_searchable() {
        let session = SavedSession {
            schema_version: 1,
            metadata: sample_metadata("t"),
            messages: vec![Message {
                role: "assistant".to_string(),
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "x".into(),
                    content: "error: connection refused on port 5432".into(),
                    is_error: Some(true),
                    content_blocks: None,
                }],
            }],
            system_prompt: None,
            context_references: Vec::new(),
            artifacts: Vec::new(),
        };
        let hit = search_one(&session, &["5432".into()]).expect("tool result searchable");
        assert_eq!(hit.matched_in, "assistant");
    }

    fn sample_metadata(title: &str) -> SessionMetadata {
        SessionMetadata {
            id: "abc123".to_string(),
            title: title.to_string(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            message_count: 1,
            total_tokens: 0,
            model: "test-model".to_string(),
            workspace: std::path::PathBuf::from("/tmp"),
            mode: None,
            cost: Default::default(),
            parent_session_id: None,
            forked_from_message_count: None,
            cumulative_turn_secs: 0,
        }
    }
}
