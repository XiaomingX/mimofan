//! Context compaction for long conversations.

use anyhow::Result;
use regex::Regex;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use crate::client::DeepSeekClient;
use crate::config::DEFAULT_TEXT_MODEL;
use crate::llm_client::LlmClient;
use crate::logging;
use crate::models::{
    CacheControl, ContentBlock, Message, MessageRequest, SystemBlock, SystemPrompt,
    context_window_for_model,
};

/// Configuration for conversation compaction behavior.
///
/// v0.8.11 simplified this from the prior token-OR-message-count trigger
/// to a token-only trigger. The
/// `message_threshold` field was removed: its only purpose was to fire
/// compaction on long sessions of small messages, which is exactly the
/// case where rewriting the V4 prefix cache is least valuable. Token
/// budget is the right signal; message count was a 128K-era heuristic.
#[derive(Debug, Clone, PartialEq)]
pub struct CompactionConfig {
    pub enabled: bool,
    pub token_threshold: usize,
    pub model: String,
    pub cache_summary: bool,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self {
            // ON BY DEFAULT since v0.8.6 (#402 P0 survivability). v0.8.64
            // resolves the user-facing default through the active model's
            // known context window, while explicit `auto_compact = false`
            // remains the opt-out. This fallback covers code paths that build
            // a `CompactionConfig` directly; real per-model values are still
            // derived through the threshold helpers.
            enabled: true,
            // v0.8.11: 50K was a 128K-era leftover that biased every
            // unconfigured caller toward "compact almost immediately on V4."
            // Bumped to 800K (80% of V4's 1M window) so the dead-code
            // default matches the hard automatic compaction guardrail. This
            // is intentionally later than the model-visible 60% "suggest
            // /compact during sustained work" guidance so automatic
            // replacement compaction stays a late continuity guardrail.
            // Real call sites override this via
            // `compaction_threshold_for_model_and_effort`.
            token_threshold: 800_000,
            model: DEFAULT_TEXT_MODEL.to_string(),
            cache_summary: true,
        }
    }
}

pub const KEEP_RECENT_MESSAGES: usize = 4;
#[allow(dead_code)]
pub const HARD_COMPACT_KEEP_RECENT: usize = 8;
const RECENT_WORKING_SET_WINDOW: usize = 12;
const MAX_WORKING_SET_PATHS: usize = 24;
const MIN_SUMMARIZE_MESSAGES: usize = 6;
const SUMMARY_TEXT_SNIPPET_CHARS: usize = 800;
const SUMMARY_TOOL_RESULT_SNIPPET_CHARS: usize = 240;
const SUMMARY_INPUT_MAX_CHARS: usize = 24_000;
const SUMMARY_INPUT_HEAD_CHARS: usize = 14_000;
const SUMMARY_INPUT_TAIL_CHARS: usize = 6_000;
const LARGE_CONTEXT_SUMMARY_TEXT_SNIPPET_CHARS: usize = 2_000;
const LARGE_CONTEXT_SUMMARY_TOOL_RESULT_SNIPPET_CHARS: usize = 4_000;
const LARGE_CONTEXT_SUMMARY_INPUT_MAX_CHARS: usize = 120_000;
const LARGE_CONTEXT_SUMMARY_INPUT_HEAD_CHARS: usize = 72_000;
const LARGE_CONTEXT_SUMMARY_INPUT_TAIL_CHARS: usize = 36_000;
const TOOL_PRUNE_STOP_CHECK_BYTES: usize = 16 * 1024;
const LARGE_CONTEXT_SUMMARY_MAX_TOKENS: u32 = 2_048;
const LARGE_CONTEXT_WINDOW_TOKENS: u32 = 500_000;
const CACHE_ALIGNED_SUMMARY_CONTEXT_BUDGET_PERCENT: usize = 85;

#[derive(Debug, Clone, Copy)]
struct SummaryInputLimits {
    text_snippet_chars: usize,
    tool_result_snippet_chars: usize,
    input_max_chars: usize,
    input_head_chars: usize,
    input_tail_chars: usize,
    max_tokens: u32,
    word_limit: usize,
}

fn summary_input_limits_for_model(model: &str) -> SummaryInputLimits {
    let is_large_context =
        context_window_for_model(model).is_some_and(|window| window >= LARGE_CONTEXT_WINDOW_TOKENS);
    if is_large_context {
        SummaryInputLimits {
            text_snippet_chars: LARGE_CONTEXT_SUMMARY_TEXT_SNIPPET_CHARS,
            tool_result_snippet_chars: LARGE_CONTEXT_SUMMARY_TOOL_RESULT_SNIPPET_CHARS,
            input_max_chars: LARGE_CONTEXT_SUMMARY_INPUT_MAX_CHARS,
            input_head_chars: LARGE_CONTEXT_SUMMARY_INPUT_HEAD_CHARS,
            input_tail_chars: LARGE_CONTEXT_SUMMARY_INPUT_TAIL_CHARS,
            max_tokens: LARGE_CONTEXT_SUMMARY_MAX_TOKENS,
            word_limit: 900,
        }
    } else {
        SummaryInputLimits {
            text_snippet_chars: SUMMARY_TEXT_SNIPPET_CHARS,
            tool_result_snippet_chars: SUMMARY_TOOL_RESULT_SNIPPET_CHARS,
            input_max_chars: SUMMARY_INPUT_MAX_CHARS,
            input_head_chars: SUMMARY_INPUT_HEAD_CHARS,
            input_tail_chars: SUMMARY_INPUT_TAIL_CHARS,
            max_tokens: 1_024,
            word_limit: 500,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct CompactionPlan {
    pub pinned_indices: BTreeSet<usize>,
    pub summarize_indices: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub struct HardCompactionConfig {
    pub enabled: bool,
    pub keep_recent: usize,
}

impl Default for HardCompactionConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            keep_recent: HARD_COMPACT_KEEP_RECENT,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub struct HardCompactionPlan {
    pub summarize_indices: Vec<usize>,
    pub preserved_indices: Vec<usize>,
}

fn path_regex() -> &'static Regex {
    static PATH_RE: OnceLock<Regex> = OnceLock::new();
    PATH_RE.get_or_init(|| {
        Regex::new(
            r"(?x)
            (?:
                (?P<root>
                    Cargo\.toml|
                    Cargo\.lock|
                    README\.md|
                    CHANGELOG\.md|
                    AGENTS\.md|
                    config\.example\.toml
                )
            )
            |
            (?P<path>
                (?:[A-Za-z0-9._-]+/)+
                [A-Za-z0-9._-]+
                \.(?:rs|toml|md|json|ya?ml|txt|lock)
            )
        ",
        )
        .expect("path regex is valid")
    })
}

fn normalize_path_candidate(candidate: &str, workspace: Option<&Path>) -> Option<String> {
    if candidate.is_empty() {
        return None;
    }

    let cleaned = candidate.replace('\\', "/");
    let mut path = PathBuf::from(cleaned);

    if path.is_absolute() {
        let ws = workspace?;
        if let Ok(stripped) = path.strip_prefix(ws) {
            path = stripped.to_path_buf();
        } else {
            return None;
        }
    }

    let rel = path.to_string_lossy().trim_start_matches("./").to_string();
    if rel.is_empty() || rel.contains("..") {
        return None;
    }

    if let Some(ws) = workspace {
        let repo_path = ws.join(&rel);
        if repo_path.exists() || looks_repo_relative(&rel) {
            return Some(rel);
        }
        return None;
    }

    if looks_repo_relative(&rel) {
        return Some(rel);
    }

    None
}

fn looks_repo_relative(path: &str) -> bool {
    matches!(
        path,
        "Cargo.toml"
            | "Cargo.lock"
            | "README.md"
            | "CHANGELOG.md"
            | "AGENTS.md"
            | "config.example.toml"
    ) || path.starts_with("src/")
        || path.starts_with("tests/")
        || path.starts_with("docs/")
        || path.starts_with("examples/")
        || path.starts_with("benches/")
        || path.starts_with("crates/")
        || path.starts_with(".github/")
        || (path.contains('/') && path.rsplit('.').next().is_some())
}

fn extract_paths_from_text(text: &str, workspace: Option<&Path>) -> Vec<String> {
    path_regex()
        .captures_iter(text)
        .filter_map(|caps| {
            let candidate = caps
                .name("path")
                .or_else(|| caps.name("root"))
                .map(|m| m.as_str())?;
            normalize_path_candidate(candidate, workspace)
        })
        .collect()
}

fn extract_paths_from_tool_input(
    input: &serde_json::Value,
    workspace: Option<&Path>,
) -> Vec<String> {
    let mut out = Vec::new();
    let Some(obj) = input.as_object() else {
        return out;
    };

    for key in ["path", "file", "target", "cwd"] {
        if let Some(val) = obj.get(key).and_then(serde_json::Value::as_str)
            && let Some(path) = normalize_path_candidate(val, workspace)
        {
            out.push(path);
        }
    }

    for key in ["paths", "files", "targets"] {
        if let Some(vals) = obj.get(key).and_then(serde_json::Value::as_array) {
            for val in vals {
                if let Some(s) = val.as_str()
                    && let Some(path) = normalize_path_candidate(s, workspace)
                {
                    out.push(path);
                }
            }
        }
    }

    out
}

fn message_text(msg: &Message) -> String {
    let mut text = String::new();
    for block in &msg.content {
        match block {
            ContentBlock::Text { text: t, .. } => {
                let _ = writeln!(text, "{t}");
            }
            ContentBlock::Thinking { .. } => {}
            ContentBlock::ToolUse { name, input, .. } => {
                let _ = writeln!(text, "[tool_use:{name}] {input}");
            }
            ContentBlock::ToolResult { content, .. } => {
                let _ = writeln!(text, "{content}");
            }
            ContentBlock::ServerToolUse { .. }
            | ContentBlock::ToolSearchToolResult { .. }
            | ContentBlock::CodeExecutionToolResult { .. }
            | ContentBlock::ImageUrl { .. } => {}
        }
    }
    text
}

fn is_user_text_query(msg: &Message) -> bool {
    msg.role == "user"
        && msg
            .content
            .iter()
            .any(|block| matches!(block, ContentBlock::Text { .. }))
}

fn extract_paths_from_message(message: &Message, workspace: Option<&Path>) -> Vec<String> {
    let mut paths = Vec::new();
    for block in &message.content {
        let candidates = match block {
            ContentBlock::Text { text, .. } => extract_paths_from_text(text, workspace),
            ContentBlock::ToolResult { content, .. } => extract_paths_from_text(content, workspace),
            ContentBlock::ToolUse { input, .. } => extract_paths_from_tool_input(input, workspace),
            ContentBlock::Thinking { .. } => Vec::new(),
            ContentBlock::ServerToolUse { .. }
            | ContentBlock::ToolSearchToolResult { .. }
            | ContentBlock::CodeExecutionToolResult { .. }
            | ContentBlock::ImageUrl { .. } => Vec::new(),
        };
        paths.extend(candidates);
    }
    paths
}

fn derive_working_set_paths(
    messages: &[Message],
    workspace: Option<&Path>,
    seed_indices: &[usize],
) -> HashSet<String> {
    let mut paths: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    let mut seeds: Vec<usize> = seed_indices
        .iter()
        .copied()
        .filter(|idx| *idx < messages.len())
        .collect();
    seeds.sort_unstable_by(|a, b| b.cmp(a));

    for idx in seeds {
        for candidate in extract_paths_from_message(&messages[idx], workspace) {
            if seen.insert(candidate.clone()) {
                paths.push(candidate);
                if paths.len() >= MAX_WORKING_SET_PATHS {
                    return paths.into_iter().collect();
                }
            }
        }
    }

    for msg in messages.iter().rev().take(RECENT_WORKING_SET_WINDOW) {
        for candidate in extract_paths_from_message(msg, workspace) {
            if seen.insert(candidate.clone()) {
                paths.push(candidate);
                if paths.len() >= MAX_WORKING_SET_PATHS {
                    return paths.into_iter().collect();
                }
            }
        }
    }

    paths.into_iter().collect()
}

fn should_pin_message(text: &str, working_set_paths: &HashSet<String>) -> bool {
    let lower = text.to_lowercase();

    let mentions_working_set = working_set_paths.iter().any(|p| text.contains(p));
    if mentions_working_set {
        return true;
    }

    let error_markers = [
        "error:",
        "error ",
        "failed",
        "panic",
        "traceback",
        "stack trace",
        "assertion failed",
        "test failed",
    ];
    if error_markers.iter().any(|m| lower.contains(m)) {
        return true;
    }

    let patch_markers = [
        "diff --git",
        "+++ b/",
        "--- a/",
        "*** begin patch",
        "*** update file:",
        "*** add file:",
        "*** delete file:",
        "```diff",
        "apply_patch",
    ];
    patch_markers.iter().any(|m| lower.contains(m))
}

pub fn plan_compaction(
    messages: &[Message],
    workspace: Option<&Path>,
    keep_recent: usize,
    external_pins: Option<&[usize]>,
    external_working_set_paths: Option<&[String]>,
) -> CompactionPlan {
    let mut pinned_indices: BTreeSet<usize> = BTreeSet::new();
    let len = messages.len();
    if len == 0 {
        return CompactionPlan::default();
    }

    // Always pin the tail of the conversation to preserve immediate context.
    let recent_start = len.saturating_sub(keep_recent);
    pinned_indices.extend(recent_start..len);

    // Derive a repo-aware working set from recent messages/tool calls and
    // merge it with any externally provided working-set paths.
    let seed_indices = external_pins.unwrap_or(&[]);
    let mut working_set_paths = derive_working_set_paths(messages, workspace, seed_indices);
    if let Some(paths) = external_working_set_paths {
        for path in paths {
            if let Some(normalized) = normalize_path_candidate(path, workspace) {
                let _ = working_set_paths.insert(normalized);
            }
        }
    }

    for (idx, msg) in messages.iter().enumerate() {
        if pinned_indices.contains(&idx) {
            continue;
        }
        let text = message_text(msg);
        if should_pin_message(&text, &working_set_paths) {
            pinned_indices.insert(idx);
        }
    }

    // External pins are authoritative and should be preserved even if they
    // were not detected by the heuristics above.
    if let Some(pins) = external_pins {
        pinned_indices.extend(pins.iter().copied().filter(|idx| *idx < len));
    }

    // Ensure tool result messages are not kept without their corresponding tool call.
    enforce_tool_call_pairs(messages, &mut pinned_indices);

    // Some OpenAI-compatible chat templates require at least one user text
    // message. Tool-heavy tails can otherwise compact down to only tool calls
    // and tool results, which makes those backends reject the next request.
    if !pinned_indices
        .iter()
        .any(|&idx| is_user_text_query(&messages[idx]))
        && let Some(idx) = messages
            .iter()
            .enumerate()
            .rev()
            .find_map(|(idx, msg)| is_user_text_query(msg).then_some(idx))
    {
        pinned_indices.insert(idx);
    }

    let summarize_indices = (0..len)
        .filter(|idx| !pinned_indices.contains(idx))
        .collect();

    // `working_set_paths` was used only for pinning decisions above.
    drop(working_set_paths);

    CompactionPlan {
        pinned_indices,
        summarize_indices,
    }
}

#[allow(dead_code)]
pub fn plan_hard_compaction(
    messages: &[Message],
    workspace: Option<&Path>,
    keep_recent: usize,
) -> Option<HardCompactionPlan> {
    if keep_recent == 0 || messages.len() < keep_recent.saturating_add(MIN_SUMMARIZE_MESSAGES) {
        return None;
    }

    let soft_plan = plan_compaction(messages, workspace, keep_recent, None, None);
    if soft_plan.summarize_indices.len() < MIN_SUMMARIZE_MESSAGES {
        return None;
    }

    let summarized: BTreeSet<_> = soft_plan.summarize_indices.iter().copied().collect();
    let preserved_indices = (0..messages.len())
        .filter(|idx| !summarized.contains(idx))
        .collect();

    Some(HardCompactionPlan {
        summarize_indices: soft_plan.summarize_indices,
        preserved_indices,
    })
}

fn enforce_tool_call_pairs(messages: &[Message], pinned_indices: &mut BTreeSet<usize>) {
    if pinned_indices.is_empty() {
        return;
    }

    // Build maps: tool_id → message index across ALL messages (not just pinned).
    let mut call_id_to_idx: HashMap<String, usize> = HashMap::new();
    let mut result_id_to_idx: HashMap<String, usize> = HashMap::new();

    for (idx, msg) in messages.iter().enumerate() {
        for block in &msg.content {
            match block {
                ContentBlock::ToolUse { id, .. } => {
                    call_id_to_idx.insert(id.clone(), idx);
                }
                ContentBlock::ToolResult { tool_use_id, .. } => {
                    result_id_to_idx.insert(tool_use_id.clone(), idx);
                }
                _ => {}
            }
        }
    }

    // Fixpoint loop: re-check until stable.
    // Newly pinned messages may introduce new pair requirements;
    // removed messages may orphan their counterparts.
    // Track permanently removed indices so they cannot be re-added
    // by a counterpart in a later iteration (prevents oscillation).
    let mut permanently_removed: HashSet<usize> = HashSet::new();

    let max_iters = messages.len().max(10);
    let mut converged = false;
    for _ in 0..max_iters {
        let mut to_add = Vec::new();
        let mut to_remove = Vec::new();

        let snapshot: Vec<usize> = pinned_indices.iter().copied().collect();

        for idx in snapshot {
            let msg = &messages[idx];
            for block in &msg.content {
                match block {
                    // Pinned result → its call must also be pinned (or remove result)
                    ContentBlock::ToolResult { tool_use_id, .. } => {
                        match call_id_to_idx.get(tool_use_id) {
                            Some(&call_idx) if !permanently_removed.contains(&call_idx) => {
                                to_add.push(call_idx);
                            }
                            _ => {
                                to_remove.push(idx);
                            }
                        }
                    }
                    // Pinned call → its result must also be pinned (or remove call)
                    ContentBlock::ToolUse { id, .. } => match result_id_to_idx.get(id) {
                        Some(&result_idx) if !permanently_removed.contains(&result_idx) => {
                            to_add.push(result_idx);
                        }
                        _ => {
                            to_remove.push(idx);
                        }
                    },
                    _ => {}
                }
            }
        }

        // Removals take priority: if a message is both needed and orphaned,
        // remove it now; the fixpoint loop will cascade the orphaning.
        let remove_set: HashSet<usize> = to_remove.iter().copied().collect();
        let mut changed = false;
        for idx in to_add {
            if !remove_set.contains(&idx) && pinned_indices.insert(idx) {
                changed = true;
            }
        }
        for idx in to_remove {
            if pinned_indices.remove(&idx) {
                permanently_removed.insert(idx);
                changed = true;
            }
        }

        if !changed {
            converged = true;
            break;
        }
    }
    if !converged {
        logging::warn(format!(
            "enforce_tool_call_pairs did not converge after {max_iters} iterations \
             ({} messages, {} pinned)",
            messages.len(),
            pinned_indices.len()
        ));
    }
}

fn estimate_tokens_for_message(message: &Message, include_thinking: bool) -> usize {
    message
        .content
        .iter()
        .map(|c| match c {
            ContentBlock::Text { text, .. } => text.len() / 4,
            // Historical reasoning blocks are UI/session metadata for DeepSeek.
            // Only current-turn tool-call reasoning is sent back to the API.
            ContentBlock::Thinking { thinking, .. } if include_thinking => thinking.len() / 4,
            ContentBlock::Thinking { .. } => 0,
            ContentBlock::ToolUse { input, .. } => serde_json::to_string(input)
                .map(|s| s.len() / 4)
                .unwrap_or(100),
            ContentBlock::ToolResult { content, .. } => content.len() / 4,
            ContentBlock::ServerToolUse { .. }
            | ContentBlock::ToolSearchToolResult { .. }
            | ContentBlock::CodeExecutionToolResult { .. }
            | ContentBlock::ImageUrl { .. } => 0,
        })
        .sum::<usize>()
}

pub fn estimate_tokens(messages: &[Message]) -> usize {
    // Rough estimate: ~4 chars per token. DeepSeek thinking-mode rule: any
    // assistant message with tool_calls keeps its reasoning_content forever
    // (replayed in all subsequent requests). Final text-only answers drop it.
    messages
        .iter()
        .map(|message| estimate_tokens_for_message(message, message_has_tool_use(message)))
        .sum()
}

fn message_has_tool_use(message: &Message) -> bool {
    message
        .content
        .iter()
        .any(|block| matches!(block, ContentBlock::ToolUse { .. }))
}

pub fn estimate_text_tokens_conservative(text: &str) -> usize {
    text.chars().count().div_ceil(3)
}

fn estimate_system_tokens_conservative(system: Option<&SystemPrompt>) -> usize {
    match system {
        Some(SystemPrompt::Text(text)) => estimate_text_tokens_conservative(text),
        Some(SystemPrompt::Blocks(blocks)) => blocks
            .iter()
            .map(|block| estimate_text_tokens_conservative(&block.text))
            .sum(),
        None => 0,
    }
}

/// Conservative estimate for full request input tokens (messages + system + framing).
#[must_use]
pub fn estimate_input_tokens_conservative(
    messages: &[Message],
    system: Option<&SystemPrompt>,
) -> usize {
    let message_tokens = estimate_tokens(messages).saturating_mul(3).div_ceil(2);
    let system_tokens = estimate_system_tokens_conservative(system);
    let framing_overhead = messages.len().saturating_mul(12).saturating_add(48);
    message_tokens
        .saturating_add(system_tokens)
        .saturating_add(framing_overhead)
}

pub fn should_compact(
    messages: &[Message],
    config: &CompactionConfig,
    workspace: Option<&Path>,
    external_pins: Option<&[usize]>,
    external_working_set_paths: Option<&[String]>,
) -> bool {
    if !config.enabled {
        return false;
    }

    let plan = plan_compaction(
        messages,
        workspace,
        KEEP_RECENT_MESSAGES,
        external_pins,
        external_working_set_paths,
    );
    let pinned_tokens: usize = plan
        .pinned_indices
        .iter()
        .map(|&idx| estimate_tokens_for_message(&messages[idx], false))
        .sum();

    let token_estimate: usize = plan
        .summarize_indices
        .iter()
        .map(|&idx| estimate_tokens_for_message(&messages[idx], false))
        .sum();
    let message_count = plan.summarize_indices.len();

    // Pinned messages consume part of the budget, so compact earlier when needed.
    let effective_token_threshold = config.token_threshold.saturating_sub(pinned_tokens);

    // Token-only trigger (v0.8.11): the prior message-count branch was a
    // 128K-era heuristic that fired compaction on long chats of small
    // messages — exactly the case where rewriting the V4 prefix cache is
    // most wasteful. Token budget is the only signal that maps to actual
    // model context pressure.
    if effective_token_threshold == 0 {
        return message_count >= MIN_SUMMARIZE_MESSAGES;
    }
    if message_count < MIN_SUMMARIZE_MESSAGES {
        return false;
    }
    token_estimate > effective_token_threshold
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

fn tail_chars(text: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let total_chars = text.chars().count();
    if total_chars <= max_chars {
        return text.to_string();
    }
    let start_char = total_chars.saturating_sub(max_chars);
    let start_idx = text
        .char_indices()
        .nth(start_char)
        .map_or(0, |(idx, _)| idx);
    text[start_idx..].to_string()
}

#[derive(Debug, Clone)]
struct ToolUseInfo {
    name: String,
    key: String,
    args_preview: String,
}

fn tool_use_key(name: &str, input: &serde_json::Value) -> String {
    format!(
        "{name}:{}",
        serde_json::to_string(input).unwrap_or_else(|_| input.to_string())
    )
}

fn tool_args_preview(input: &serde_json::Value) -> String {
    let raw = serde_json::to_string(input).unwrap_or_else(|_| input.to_string());
    truncate_chars(&raw, 120).to_string()
}

fn collect_tool_uses(messages: &[Message]) -> HashMap<String, ToolUseInfo> {
    let mut tool_uses = HashMap::new();
    for message in messages {
        for block in &message.content {
            if let ContentBlock::ToolUse {
                id, name, input, ..
            } = block
            {
                tool_uses.insert(
                    id.clone(),
                    ToolUseInfo {
                        name: name.clone(),
                        key: tool_use_key(name, input),
                        args_preview: tool_args_preview(input),
                    },
                );
            }
        }
    }
    tool_uses
}

struct ToolResultPruneCandidate {
    message_idx: usize,
    block_idx: usize,
    key: String,
    tool_name: String,
    args_preview: String,
    original_len: usize,
}

/// Mechanically prune old verbose tool results before paying for an LLM summary.
///
/// The most recent `protected_window` messages stay byte-for-byte intact. Older
/// duplicate tool results keep the freshest full body and replace earlier
/// copies with one-line summaries; non-duplicate old results are summarized only
/// when they exceed the normal summary snippet size.
fn prune_tool_results_until<F>(
    messages: &mut [Message],
    protected_window: usize,
    mut should_stop: F,
) -> usize
where
    F: FnMut(&[Message], usize) -> bool,
{
    let cutoff = messages.len().saturating_sub(protected_window);
    if cutoff == 0 {
        return 0;
    }

    let tool_uses = collect_tool_uses(messages);
    let mut candidates = Vec::new();
    let mut latest_by_key: HashMap<String, usize> = HashMap::new();
    let mut count_by_key: HashMap<String, usize> = HashMap::new();

    for (message_idx, message) in messages.iter().take(cutoff).enumerate() {
        for (block_idx, block) in message.content.iter().enumerate() {
            let ContentBlock::ToolResult {
                tool_use_id,
                content,
                ..
            } = block
            else {
                continue;
            };
            let Some(info) = tool_uses.get(tool_use_id) else {
                continue;
            };
            latest_by_key.insert(info.key.clone(), message_idx);
            *count_by_key.entry(info.key.clone()).or_insert(0) += 1;
            candidates.push(ToolResultPruneCandidate {
                message_idx,
                block_idx,
                key: info.key.clone(),
                tool_name: info.name.clone(),
                args_preview: info.args_preview.clone(),
                original_len: content.len(),
            });
        }
    }

    // The maps above are fully populated before pruning starts, so the order below
    // only changes which message bytes are rewritten first. Pruning from newest to
    // oldest lets callers stop as soon as enough bytes were saved, preserving the
    // earlier JSON request prefix for byte-level KV caches.
    candidates.reverse();

    let mut bytes_saved = 0usize;
    for candidate in candidates {
        let duplicate_count = count_by_key.get(&candidate.key).copied().unwrap_or(0);
        let is_latest_duplicate = duplicate_count > 1
            && latest_by_key.get(&candidate.key) == Some(&candidate.message_idx);
        if is_latest_duplicate {
            continue;
        }
        if duplicate_count <= 1 && candidate.original_len <= SUMMARY_TOOL_RESULT_SNIPPET_CHARS {
            continue;
        }

        let summary = format!(
            "[{}] tool result pruned ({} bytes; args: {})",
            candidate.tool_name, candidate.original_len, candidate.args_preview
        );
        if summary.len() >= candidate.original_len {
            continue;
        }

        if let ContentBlock::ToolResult {
            content,
            content_blocks,
            ..
        } = &mut messages[candidate.message_idx].content[candidate.block_idx]
        {
            bytes_saved = bytes_saved.saturating_add(content.len().saturating_sub(summary.len()));
            *content = summary;
            *content_blocks = None;

            if should_stop(messages, bytes_saved) {
                break;
            }
        }
    }

    bytes_saved
}

/// Result of a compaction operation with metadata.
#[derive(Debug)]
pub struct CompactionResult {
    /// Compacted messages
    pub messages: Vec<Message>,
    /// Summary system prompt
    pub summary_prompt: Option<SystemPrompt>,
    /// Messages that were removed from the active window
    #[allow(dead_code)]
    pub removed_messages: Vec<Message>,
    /// Number of retries used before success
    pub retries_used: u32,
}

/// Check if an error is transient and worth retrying. Categories that map to
/// transient retry: Network, RateLimit, Timeout. Anything else (auth, parse,
/// invalid request, etc.) is permanent and propagates.
fn is_transient_error(e: &anyhow::Error) -> bool {
    let category = crate::error_taxonomy::classify_error_message(&e.to_string());
    matches!(
        category,
        crate::error_taxonomy::ErrorCategory::Network
            | crate::error_taxonomy::ErrorCategory::RateLimit
            | crate::error_taxonomy::ErrorCategory::Timeout
    )
}

/// Compact messages with retry and backoff for transient errors.
///
/// This function wraps `compact_messages` with retry logic to handle
/// transient network errors and rate limits. It uses exponential backoff
/// with delays of 1s, 2s, 4s between retries.
///
/// # Safety
/// - Never panics
/// - Never corrupts the original messages (returns error instead)
/// - Only retries on transient errors (network, rate limit, etc.)
pub async fn compact_messages_safe(
    client: &DeepSeekClient,
    messages: &[Message],
    config: &CompactionConfig,
    workspace: Option<&Path>,
    external_pins: Option<&[usize]>,
    external_working_set_paths: Option<&[String]>,
) -> Result<CompactionResult> {
    const MAX_RETRIES: u32 = 3;
    const BASE_DELAY_MS: u64 = 1000;

    let was_over_threshold = should_compact(
        messages,
        config,
        workspace,
        external_pins,
        external_working_set_paths,
    );
    let mut pruned_messages = messages.to_vec();
    let mut now_under_threshold = false;
    let mut next_stop_check_bytes = 0usize;
    let pruned_bytes = prune_tool_results_until(
        &mut pruned_messages,
        KEEP_RECENT_MESSAGES,
        |candidate_messages, bytes_saved| {
            if !was_over_threshold || bytes_saved < next_stop_check_bytes {
                return false;
            }

            // Stop at the first suffix-side prune check that clears the threshold.
            // The check itself is a full compaction-plan pass, so bound it by saved
            // bytes instead of running it after every candidate in huge sessions.
            next_stop_check_bytes = bytes_saved.saturating_add(TOOL_PRUNE_STOP_CHECK_BYTES);
            now_under_threshold = !should_compact(
                candidate_messages,
                config,
                workspace,
                external_pins,
                external_working_set_paths,
            );
            now_under_threshold
        },
    );
    if was_over_threshold && pruned_bytes > 0 && !now_under_threshold {
        // The throttled in-loop check may skip the exact candidate that clears the
        // budget. Do one final pass so a successful local prune still avoids LLM compaction.
        now_under_threshold = !should_compact(
            &pruned_messages,
            config,
            workspace,
            external_pins,
            external_working_set_paths,
        );
    }

    let compaction_input: &[Message] = if pruned_bytes > 0 {
        logging::info(format!(
            "Local tool-result prune saved {pruned_bytes} bytes before LLM compaction"
        ));
        if was_over_threshold && now_under_threshold {
            return Ok(CompactionResult {
                messages: pruned_messages,
                summary_prompt: None,
                removed_messages: Vec::new(),
                retries_used: 0,
            });
        }
        &pruned_messages
    } else {
        messages
    };

    let mut last_error: Option<anyhow::Error> = None;

    for attempt in 0..MAX_RETRIES {
        if attempt > 0 {
            // Exponential backoff: 1s, 2s, 4s
            let delay = Duration::from_millis(BASE_DELAY_MS * (1 << (attempt - 1)));
            tokio::time::sleep(delay).await;
        }

        match compact_messages(
            client,
            compaction_input,
            config,
            workspace,
            external_pins,
            external_working_set_paths,
        )
        .await
        {
            Ok((msgs, prompt, removed)) => {
                return Ok(CompactionResult {
                    messages: msgs,
                    summary_prompt: prompt,
                    removed_messages: removed,
                    retries_used: attempt,
                });
            }
            Err(e) => {
                // Only retry on transient errors
                if !is_transient_error(&e) {
                    return Err(e);
                }
                last_error = Some(e);
            }
        }
    }

    Err(last_error
        .unwrap_or_else(|| anyhow::anyhow!("Compaction failed after {MAX_RETRIES} retries")))
}

fn read_workspace_anchors(workspace: Option<&Path>) -> Vec<String> {
    let Some(ws) = workspace else {
        return Vec::new();
    };

    // Prefer .mimofan, fall back to .deepseek
    let primary = ws.join(".mimofan").join("anchors.md");
    let anchors_path = if primary.exists() {
        primary
    } else {
        ws.join(".deepseek").join("anchors.md")
    };
    let Ok(content) = std::fs::read_to_string(anchors_path) else {
        return Vec::new();
    };

    content
        .split("\n---\n")
        .map(str::trim)
        .filter(|anchor| !anchor.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn anchor_summary_section(workspace: Option<&Path>) -> String {
    let anchors = read_workspace_anchors(workspace);
    if anchors.is_empty() {
        return String::new();
    }

    let mut section = String::from(
        "## Pinned Facts (User Anchors)\n\n\
         The following facts were explicitly anchored by the user with `/anchor`. \
         Preserve them across compaction cycles.\n\n",
    );

    for anchor in anchors {
        let _ = writeln!(section, "- {anchor}");
    }

    section.push_str("\n---\n\n");
    section
}

pub async fn compact_messages(
    client: &DeepSeekClient,
    messages: &[Message],
    config: &CompactionConfig,
    workspace: Option<&Path>,
    external_pins: Option<&[usize]>,
    external_working_set_paths: Option<&[String]>,
) -> Result<(Vec<Message>, Option<SystemPrompt>, Vec<Message>)> {
    if messages.is_empty() {
        return Ok((Vec::new(), None, Vec::new()));
    }

    let plan = plan_compaction(
        messages,
        workspace,
        KEEP_RECENT_MESSAGES,
        external_pins,
        external_working_set_paths,
    );
    if plan.summarize_indices.is_empty() {
        return Ok((messages.to_vec(), None, Vec::new()));
    }

    let to_summarize: Vec<Message> = plan
        .summarize_indices
        .iter()
        .map(|&idx| messages[idx].clone())
        .collect();

    // Create a summary of the unpinned portion of the conversation
    let summary = create_summary(client, &to_summarize, &config.model).await?;

    // Extract workflow context (files touched, tasks in progress, etc.)
    let workflow_context = extract_workflow_context(&to_summarize, workspace);

    let anchors_section = anchor_summary_section(workspace);

    // Build new message list with enhanced summary as system block
    let summary_block = SystemBlock {
        block_type: "text".to_string(),
        text: format!(
            "{anchors_section}\
             ## 📋 Conversation Summary (Auto-Generated)\n\n\
             {summary}\n\n\
             ---\n\n\
             ## 🔍 Workflow Context\n\n\
             {workflow_context}\n\n\
             ---\n\n\
             ## 💡 What to Do Next\n\n\
             You have just resumed from a context compaction. The conversation above was summarized to save space. \
             Review the summary and workflow context, then continue helping the user with their task. \
             If you need more details about the summarized portion, ask the user to clarify.\n\n\
             ---\n\n\
             Pinned messages follow:"
        ),
        cache_control: if config.cache_summary {
            Some(CacheControl {
                cache_type: "ephemeral".to_string(),
            })
        } else {
            None
        },
    };

    let pinned_messages = messages
        .iter()
        .enumerate()
        .filter_map(|(idx, msg)| plan.pinned_indices.contains(&idx).then_some(msg.clone()))
        .collect();

    Ok((
        pinned_messages,
        Some(SystemPrompt::Blocks(vec![summary_block])),
        to_summarize,
    ))
}

async fn create_summary(
    client: &DeepSeekClient,
    messages: &[Message],
    model: &str,
) -> Result<String> {
    let limits = summary_input_limits_for_model(model);
    let used_cache_aligned = should_use_cache_aligned_summary(model, messages);
    let request = if used_cache_aligned {
        build_cache_aligned_summary_request(model, messages, limits)
    } else {
        build_formatted_summary_request(model, messages, limits)
    };

    let mut telemetry_cache_aligned = used_cache_aligned;
    let response = match client.create_message(request).await {
        Ok(response) => response,
        Err(err) if used_cache_aligned && is_context_window_error(&err) => {
            logging::warn(format!(
                "Cache-aligned compaction summary exceeded the model context window ({err}); \
                 retrying with bounded formatted summary input"
            ));
            telemetry_cache_aligned = false;
            let fallback_request = build_formatted_summary_request(model, messages, limits);
            client.create_message(fallback_request).await?
        }
        Err(err) => return Err(err),
    };
    // Compaction summary calls are billed by DeepSeek; route the
    // tokens through the side-channel so the dashboard total
    // matches the website (#526).
    crate::cost_status::report(&response.model, &response.usage);

    // #584: emit one debug-level event per summary call so the
    // V4 cache-aligned win is observable post-deploy without
    // adding UI surface. The event is emitted with
    // `target = "compaction"`, so the filter is
    // `RUST_LOG=compaction=debug` (the module-path form
    // `mimofan_tui::compaction=debug` does NOT match — `EnvFilter`
    // matches the explicit target string when one is set).
    log_summary_cache_telemetry(telemetry_cache_aligned, &response.usage);

    // Extract text from response
    let summary = response
        .content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");

    Ok(summary)
}

fn is_context_window_error(e: &anyhow::Error) -> bool {
    let text = e.to_string();
    if crate::error_taxonomy::classify_error_message(&text)
        != crate::error_taxonomy::ErrorCategory::InvalidInput
    {
        return false;
    }

    let lower = text.to_lowercase();
    lower.contains("context")
        || lower.contains("token")
        || lower.contains("prompt is too long")
        || lower.contains("requested")
        || lower.contains("maximum")
}

/// Cache-hit percentage for a compaction summary call.
///
/// Denominator is `input_tokens` (the total prompt size), not
/// `cache_hit + cache_miss`. Some providers populate
/// `prompt_cache_hit_tokens` but not `prompt_cache_miss_tokens` — using
/// the sum as the denominator there reports an inflated 100% even when
/// most of the prompt was uncached. Anchoring on `input_tokens` matches
/// how the rest of the codebase (cost reporting, `/cache`) infers
/// missing miss counts. (#584)
fn summary_cache_hit_percent(cache_hit: u32, input_tokens: u32) -> f64 {
    if input_tokens > 0 {
        (f64::from(cache_hit) * 100.0) / f64::from(input_tokens)
    } else {
        0.0
    }
}

/// Emit one `tracing::debug!` event per compaction summary call so the
/// path choice (cache-aligned vs fallback) and the resulting cache-hit
/// rate are observable. Both raw token counts and the percentage are
/// included; on providers that don't return cache-token fields the
/// counts are reported as `0` and the percentage as `0.0`. (#584)
fn log_summary_cache_telemetry(used_cache_aligned: bool, usage: &crate::models::Usage) {
    let path = if used_cache_aligned {
        "cache_aligned"
    } else {
        "fallback"
    };
    let cache_hit = usage.prompt_cache_hit_tokens.unwrap_or(0);
    let cache_miss = usage.prompt_cache_miss_tokens.unwrap_or(0);
    let cache_hit_pct = summary_cache_hit_percent(cache_hit, usage.input_tokens);
    tracing::debug!(
        target: "compaction",
        "compaction summary call: path={} prompt_tokens={} cache_hit_tokens={} cache_miss_tokens={} cache_hit_pct={:.1}",
        path,
        usage.input_tokens,
        cache_hit,
        cache_miss,
        cache_hit_pct,
    );
}

/// Decide whether to use the cache-aligned summary path
/// ([`build_cache_aligned_summary_request`]) or the fallback
/// ([`build_formatted_summary_request`]). Returns `true` when both
/// gates hold:
///
/// 1. The model has a known large context window
///    (≥ `LARGE_CONTEXT_WINDOW_TOKENS`, currently V4-scale).
/// 2. Replaying the message prefix plus a ~512-token instruction
///    still fits within `CACHE_ALIGNED_SUMMARY_CONTEXT_BUDGET_PERCENT`
///    of that budget.
///
/// ## Why the two paths produce slightly different prompts (#584)
///
/// The two summary requests are *intentionally* framed differently:
///
/// - **Cache-aligned** replays the original `messages` verbatim
///   with `system: None` and appends the summary instruction as
///   the final `user` turn. The model sees the conversation as if
///   it were its own history. This is what lets the V4 prefix cache
///   hit on the bulk of the request (#572).
/// - **Fallback** reformats the conversation into a flat
///   `User:/Assistant:` transcript inside a single `user` message
///   and adds a "You are a helpful assistant that creates concise
///   conversation summaries." system prompt. The model sees a
///   transcript of someone else's conversation.
///
/// The empirical bar is that V4 produces equivalent summaries
/// either way; the post-#572 review noted this fork is worth
/// documenting but not yet worth unifying. The fallback's
/// external-transcript framing is also more conservative for the
/// older / smaller models the cache-aligned path explicitly
/// excludes, so dropping the system prompt would risk regressing
/// those models without a corresponding gain. If we ever want to
/// unify, land it in a separate PR backed by an A/B summary-quality
/// evaluation rather than as a drive-by cleanup.
///
/// `create_summary` emits a `tracing::debug!` event under
/// `target = "compaction"` after each call so the path choice and
/// cache-hit rate are observable post-deploy without UI surface.
fn should_use_cache_aligned_summary(model: &str, messages: &[Message]) -> bool {
    let Some(window) = context_window_for_model(model) else {
        return false;
    };
    if window < LARGE_CONTEXT_WINDOW_TOKENS {
        return false;
    }

    let budget = usize::try_from(window).unwrap_or(usize::MAX)
        * CACHE_ALIGNED_SUMMARY_CONTEXT_BUDGET_PERCENT
        / 100;
    let summary_prompt_tokens = 512usize;
    estimate_tokens(messages).saturating_add(summary_prompt_tokens) <= budget
}

fn summary_instruction(word_limit: usize) -> String {
    format!(
        "Summarize the conversation above in a concise but comprehensive way. \
         Preserve key information, decisions made, exact file paths, commands, \
         errors, and tool-result facts needed to continue the work. \
         Tool outputs may be abbreviated only when they are repetitive. \
         Keep it under {word_limit} words."
    )
}

fn build_cache_aligned_summary_request(
    model: &str,
    messages: &[Message],
    limits: SummaryInputLimits,
) -> MessageRequest {
    let mut request_messages = messages.to_vec();
    request_messages.push(Message {
        role: "user".to_string(),
        content: vec![ContentBlock::Text {
            text: summary_instruction(limits.word_limit),
            cache_control: None,
        }],
    });

    MessageRequest {
        model: model.to_string(),
        messages: request_messages,
        max_tokens: limits.max_tokens,
        system: None,
        tools: None,
        tool_choice: None,
        metadata: None,
        thinking: None,
        reasoning_effort: None,
        stream: Some(false),
        temperature: Some(0.3),
        top_p: None,
        response_format: None,
    }
}

fn build_formatted_summary_request(
    model: &str,
    messages: &[Message],
    limits: SummaryInputLimits,
) -> MessageRequest {
    // Format messages for summarization
    let mut conversation_text = String::new();
    for msg in messages {
        let role = if msg.role == "user" {
            "User"
        } else {
            "Assistant"
        };
        for block in &msg.content {
            match block {
                ContentBlock::Text { text, .. } => {
                    let snippet = truncate_chars(text, limits.text_snippet_chars);
                    let _ = write!(conversation_text, "{role}: {snippet}\n\n");
                }
                ContentBlock::ToolUse { name, .. } => {
                    let _ = write!(conversation_text, "{role}: [Used tool: {name}]\n\n");
                }
                ContentBlock::ToolResult { content, .. } => {
                    let snippet = truncate_chars(content, limits.tool_result_snippet_chars);
                    let _ = write!(conversation_text, "Tool result: {snippet}\n\n");
                }
                ContentBlock::Thinking { .. } => {
                    // Skip thinking blocks in summary
                }
                ContentBlock::ServerToolUse { .. }
                | ContentBlock::ToolSearchToolResult { .. }
                | ContentBlock::CodeExecutionToolResult { .. }
                | ContentBlock::ImageUrl { .. } => {}
            }
        }
    }

    let conversation_chars = conversation_text.chars().count();
    if conversation_chars > limits.input_max_chars {
        let head = truncate_chars(&conversation_text, limits.input_head_chars).to_string();
        let tail = tail_chars(&conversation_text, limits.input_tail_chars);
        let omitted = conversation_chars
            .saturating_sub(head.chars().count())
            .saturating_sub(tail.chars().count());
        conversation_text =
            format!("{head}\n\n[... {omitted} characters omitted before summary ...]\n\n{tail}");
    }

    MessageRequest {
        model: model.to_string(),
        messages: vec![Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: format!(
                    "{}\n\n---\n\n{conversation_text}",
                    summary_instruction(limits.word_limit)
                ),
                cache_control: None,
            }],
        }],
        max_tokens: limits.max_tokens,
        system: Some(SystemPrompt::Text(
            include_str!("prompts/conversation_summary.md")
                .trim()
                .to_string(),
        )),
        tools: None,
        tool_choice: None,
        metadata: None,
        thinking: None,
        reasoning_effort: None,
        stream: Some(false),
        temperature: Some(0.3),
        top_p: None,
        response_format: None,
    }
}

/// Extract workflow context from messages (files touched, tasks, etc.)
fn extract_workflow_context(messages: &[Message], workspace: Option<&Path>) -> String {
    let mut files_touched: Vec<String> = Vec::new();
    let mut tools_used: Vec<String> = Vec::new();
    let mut tasks_identified: Vec<String> = Vec::new();

    for msg in messages {
        for block in &msg.content {
            match block {
                ContentBlock::ToolUse { name, input, .. } => {
                    tools_used.push(name.clone());

                    // Extract file paths from tool inputs
                    if let Some(path) = extract_path_from_input(input)
                        && !files_touched.contains(&path)
                    {
                        files_touched.push(path);
                    }
                }
                ContentBlock::Text { text, .. }
                    // Look for task/todo mentions
                    if (text.contains("TODO") || text.contains("task") || text.contains("need to")) => {
                        let task = truncate_chars(text, 200).to_string();
                        if !tasks_identified.contains(&task) {
                            tasks_identified.push(task);
                        }
                    }
                _ => {}
            }
        }
    }

    let mut context = String::new();

    if !files_touched.is_empty() {
        context.push_str("**Files Modified/Read:**\n");
        for file in &files_touched {
            if let Some(ws) = workspace {
                let relative = Path::new(file)
                    .strip_prefix(ws)
                    .unwrap_or(Path::new(file))
                    .display();
                context.push_str(&format!("- `{relative}`\n"));
            } else {
                context.push_str(&format!("- `{file}`\n"));
            }
        }
        context.push('\n');
    }

    if !tools_used.is_empty() {
        context.push_str("**Tools Used:** ");
        context.push_str(&tools_used.join(", "));
        context.push_str("\n\n");
    }

    if !tasks_identified.is_empty() {
        context.push_str("**Tasks/TODOs Identified:**\n");
        for task in &tasks_identified {
            context.push_str(&format!("- {task}\n"));
        }
        context.push('\n');
    }

    if context.is_empty() {
        context.push_str("No specific workflow context detected. Continue assisting the user with their current task.\n");
    }

    context
}

/// Extract file path from tool input JSON
fn extract_path_from_input(input: &serde_json::Value) -> Option<String> {
    // Try common path field names
    for key in ["path", "file", "file_path", "filename"] {
        if let Some(path) = input.get(key).and_then(|v| v.as_str()) {
            return Some(path.to_string());
        }
    }

    // Try to find path in nested objects
    if let Some(obj) = input.as_object() {
        for (_, value) in obj {
            if let Some(path) = value.as_str()
                && (path.contains('/') || path.contains('\\') || path.contains('.'))
            {
                return Some(path.to_string());
            }
        }
    }

    None
}

pub fn merge_system_prompts(
    original: Option<&SystemPrompt>,
    summary: Option<SystemPrompt>,
) -> Option<SystemPrompt> {
    match (original, summary) {
        (None, None) => None,
        (Some(orig), None) => Some(orig.clone()),
        (None, Some(sum)) => Some(sum),
        (Some(SystemPrompt::Text(orig_text)), Some(SystemPrompt::Blocks(mut sum_blocks))) => {
            // Prepend original system prompt
            sum_blocks.insert(
                0,
                SystemBlock {
                    block_type: "text".to_string(),
                    text: orig_text.clone(),
                    cache_control: None,
                },
            );
            Some(SystemPrompt::Blocks(sum_blocks))
        }
        (Some(SystemPrompt::Blocks(orig_blocks)), Some(SystemPrompt::Blocks(mut sum_blocks))) => {
            // Prepend original blocks
            for (i, block) in orig_blocks.iter().enumerate() {
                sum_blocks.insert(i, block.clone());
            }
            Some(SystemPrompt::Blocks(sum_blocks))
        }
        (Some(orig), Some(SystemPrompt::Text(sum_text))) => {
            let mut blocks = match orig {
                SystemPrompt::Text(t) => vec![SystemBlock {
                    block_type: "text".to_string(),
                    text: t.clone(),
                    cache_control: None,
                }],
                SystemPrompt::Blocks(b) => b.clone(),
            };
            blocks.push(SystemBlock {
                block_type: "text".to_string(),
                text: sum_text,
                cache_control: None,
            });
            Some(SystemPrompt::Blocks(blocks))
        }
    }
}

#[cfg(test)]
mod tests {}
