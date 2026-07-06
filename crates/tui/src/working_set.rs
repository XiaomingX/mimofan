//! Repo-aware working set tracking and prompt context packing.
//!
//! The goal of this module is to keep a small, high-signal list of
//! "active" paths that the assistant should prioritize. It observes
//! user messages and tool calls, extracts likely paths, and produces:
//! - a compact working-set summary block for the system prompt
//! - pinned message indices that compaction should preserve

use crate::models::{ContentBlock, Message};
use crate::workspace_discovery::{
    DISCOVERY_ALWAYS_DIRS, path_is_excluded_from_discovery, should_skip_unignored_discovery_entry,
};
use ignore::WalkBuilder;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

/// Repo-aware resolver for `@`-mentions and file pickers.
///
/// `cwd` is captured at construction; if the host's current directory changes
/// during a session, build a fresh `Workspace`. Fuzzy lookups are backed by a
/// lazy basename → paths index built once on first miss and reused for the
/// rest of the session — without it, every mis-typed mention triggered a full
/// `WalkBuilder` traversal up to the configured completion depth.
#[derive(Debug)]
pub struct Workspace {
    pub root: PathBuf,
    cwd: Option<PathBuf>,
    file_index: OnceLock<HashMap<String, Vec<PathBuf>>>,
    completion_walk_depth: Option<usize>,
    /// Follow symbolic links during file discovery walks. When `true`,
    /// symlinked directories are traversed, enabling multi-project workspaces
    /// where project directories are symlinked into a hub directory.
    follow_links: bool,
}

struct SearchContext<'a> {
    needle: &'a str,
    limit: usize,
    prefix_hits: &'a mut Vec<String>,
    substring_hits: &'a mut Vec<String>,
    seen: &'a mut HashSet<PathBuf>,
}

impl SearchContext<'_> {
    fn is_full(&self) -> bool {
        self.prefix_hits.len() + self.substring_hits.len() >= self.limit
    }

    fn remember(&mut self, path: PathBuf) -> bool {
        self.seen.insert(path)
    }

    fn push_match(&mut self, candidate: String) {
        let lower = candidate.to_lowercase();
        if self.needle.is_empty() || lower.starts_with(self.needle) {
            self.prefix_hits.push(candidate);
        } else if lower.contains(self.needle) {
            self.substring_hits.push(candidate);
        }
    }
}

impl Workspace {
    /// Construct a workspace anchored at `root`, capturing the process CWD as
    /// the secondary resolution pass. Convenience entry point intended for
    /// callers that don't already have a CWD on hand; the App routes through
    /// [`Workspace::with_cwd`] with its own captured launch directory.
    #[allow(dead_code)] // Keeps the surface stable for #97 (Ctrl+P picker).
    pub fn new(root: PathBuf) -> Self {
        Self::with_cwd(root, std::env::current_dir().ok())
    }

    /// Construct with an explicit cwd. Used by tests that need deterministic
    /// resolution against a known directory without depending on (and
    /// mutating) the process's real working directory.
    pub fn with_cwd(root: PathBuf, cwd: Option<PathBuf>) -> Self {
        Self::with_cwd_and_depth(root, cwd, DEFAULT_COMPLETIONS_WALK_DEPTH)
    }

    /// Construct with an explicit completion walk depth. A depth of `0`
    /// disables the depth limit for users with deeply nested workspaces.
    pub fn with_cwd_and_depth(root: PathBuf, cwd: Option<PathBuf>, walk_depth: usize) -> Self {
        Self::with_cwd_depth_and_follow_links(root, cwd, walk_depth, false)
    }

    /// Construct with an explicit completion walk depth and symlink-following
    /// preference. See [`Workspace::follow_links`].
    pub fn with_cwd_depth_and_follow_links(
        root: PathBuf,
        cwd: Option<PathBuf>,
        walk_depth: usize,
        follow_links: bool,
    ) -> Self {
        Self {
            root,
            cwd,
            file_index: OnceLock::new(),
            completion_walk_depth: normalize_completion_walk_depth(walk_depth),
            follow_links,
        }
    }

    /// Two-pass resolution: workspace, then cwd, then fuzzy fallback.
    pub fn resolve(&self, raw_path: &str) -> Result<PathBuf, PathBuf> {
        let path = expand_mention_home(raw_path);
        if path.is_absolute() {
            if path.exists() {
                return Ok(path);
            }
            return Err(path);
        }

        let ws_path = self.root.join(&path);
        if ws_path.exists() {
            return Ok(ws_path);
        }

        if let Some(cwd) = self.cwd.as_ref() {
            let cwd_path = cwd.join(&path);
            if cwd_path.exists() {
                return Ok(cwd_path);
            }
        }

        if let Some(fuzzy) = self.fuzzy_resolve(&path) {
            return Ok(fuzzy);
        }

        Err(ws_path)
    }

    fn fuzzy_resolve(&self, path: &Path) -> Option<PathBuf> {
        let needle = path.file_name()?.to_string_lossy().to_lowercase();
        if needle.is_empty() {
            return None;
        }

        let index = self.file_index.get_or_init(|| self.build_file_index());
        index.get(&needle).and_then(|paths| paths.first()).cloned()
    }

    fn build_file_index(&self) -> HashMap<String, Vec<PathBuf>> {
        let mut index: HashMap<String, Vec<PathBuf>> = HashMap::new();
        let mut total: usize = 0;
        let builder =
            discovery_walk_builder(&self.root, self.completion_walk_depth, self.follow_links);

        for entry in builder.build().flatten() {
            if total >= FILE_INDEX_MAX_ENTRIES {
                tracing::warn!(
                    target: "working_set",
                    limit = FILE_INDEX_MAX_ENTRIES,
                    "file-index discovery hit the entry cap; truncating to keep first-turn latency bounded (#697)"
                );
                return index;
            }
            if entry
                .file_type()
                .is_some_and(|ft| ft.is_file() || ft.is_dir())
            {
                let name = entry.file_name().to_string_lossy().to_lowercase();
                index
                    .entry(name)
                    .or_default()
                    .push(entry.path().to_path_buf());
                total += 1;
            }
        }

        // Also index AI-tool dot-directories with gitignore disabled.
        for dir_name in DISCOVERY_ALWAYS_DIRS {
            if total >= FILE_INDEX_MAX_ENTRIES {
                break;
            }
            let dot_dir = self.root.join(dir_name);
            if !dot_dir.is_dir() {
                continue;
            }
            let mut dot_builder = WalkBuilder::new(&dot_dir);
            dot_builder
                .hidden(true)
                .follow_links(self.follow_links)
                .git_ignore(false)
                .ignore(false);
            if let Some(depth) = child_completion_walk_depth(self.completion_walk_depth) {
                dot_builder.max_depth(Some(depth));
            }
            for entry in dot_builder.build().flatten() {
                if total >= FILE_INDEX_MAX_ENTRIES {
                    break;
                }
                // Exclude machine-generated bulk (e.g. .mimofan/snapshots/).
                if path_is_excluded_from_discovery(&self.root, entry.path()) {
                    continue;
                }
                if entry
                    .file_type()
                    .is_some_and(|ft| ft.is_file() || ft.is_dir())
                {
                    let name = entry.file_name().to_string_lossy().to_lowercase();
                    index
                        .entry(name)
                        .or_default()
                        .push(entry.path().to_path_buf());
                    total += 1;
                }
            }
        }

        // Beyond the curated dot-dir whitelist above, also index any explicit
        // hidden/ignored path the user might `@`-mention (e.g. a project's
        // own `.generated/specs/`). `local_reference_paths` walks with
        // gitignore disabled but still honors `.deepseekignore`.
        for path in local_reference_paths(
            &self.root,
            LOCAL_REFERENCE_SCAN_LIMIT,
            self.completion_walk_depth,
            self.follow_links,
        ) {
            if total >= FILE_INDEX_MAX_ENTRIES {
                break;
            }
            let Some(name) = path
                .file_name()
                .map(|name| name.to_string_lossy().to_lowercase())
            else {
                continue;
            };
            index.entry(name).or_default().push(path);
            total += 1;
        }
        index
    }

    /// Walk the workspace (and the recorded `cwd` when it diverges) and
    /// return relative paths whose representation matches `partial`.
    ///
    /// Ranking: a candidate matches when its case-insensitive display string
    /// starts with `partial` (prefix hit) or contains it as a substring; prefix
    /// hits sort first so `docs/de` lands `docs/deepseek_v4.pdf` ahead of any
    /// path that merely shares those bytes.
    ///
    /// Display strings are workspace-relative for files under `root`, and
    /// cwd-relative for files only under the recorded `cwd` — so what the user
    /// Tab-completes matches what their shell would have shown them.
    ///
    /// Honors `.gitignore`, `.git/info/exclude`, `.ignore`, and
    /// `.deepseekignore`. Capped at `limit` results.
    #[must_use]
    pub fn completions(&self, partial: &str, limit: usize) -> Vec<String> {
        if limit == 0 {
            return Vec::new();
        }
        let needle = partial.to_lowercase();
        let mut prefix_hits: Vec<String> = Vec::new();
        let mut substring_hits: Vec<String> = Vec::new();
        let mut seen: HashSet<PathBuf> = HashSet::new();

        // Walk the recorded cwd first when it diverges from the workspace
        // root, so cwd-relative entries appear ahead of duplicates surfaced by
        // the workspace walk.
        {
            let mut ctx = SearchContext {
                needle: &needle,
                limit,
                prefix_hits: &mut prefix_hits,
                substring_hits: &mut substring_hits,
                seen: &mut seen,
            };

            let cwd_diverges = self
                .cwd
                .as_deref()
                .map(|c| c != self.root.as_path())
                .unwrap_or(false);
            if cwd_diverges && let Some(cwd) = self.cwd.as_deref() {
                walk_for_completions(
                    cwd,
                    cwd,
                    &mut ctx,
                    self.completion_walk_depth,
                    self.follow_links,
                );
                add_local_reference_completions(
                    cwd,
                    cwd,
                    &mut ctx,
                    self.completion_walk_depth,
                    self.follow_links,
                );
            }
            walk_for_completions(
                &self.root,
                &self.root,
                &mut ctx,
                self.completion_walk_depth,
                self.follow_links,
            );
            add_local_reference_completions(
                &self.root,
                &self.root,
                &mut ctx,
                self.completion_walk_depth,
                self.follow_links,
            );
        }

        prefix_hits.sort();
        substring_hits.sort();
        prefix_hits.extend(substring_hits);
        prefix_hits.truncate(limit);
        prefix_hits
    }

    /// Deterministic directory-browser completions for `@` mentions.
    ///
    /// Unlike [`Workspace::completions`], this mode does not fuzzy-rank across
    /// the full workspace. It locks onto the directory part of `partial` and
    /// returns only that directory's immediate children in case-insensitive
    /// alphabetical order.
    #[must_use]
    pub fn browser_completions(&self, partial: &str, limit: usize) -> Vec<String> {
        if limit == 0 {
            return Vec::new();
        }

        let normalized = partial.replace('\\', "/");
        let trimmed = normalized.trim_start_matches('/');
        let (dir_part, name_part) = match trimmed.rsplit_once('/') {
            Some((dir, name)) => (dir.trim_end_matches('/'), name),
            None => ("", trimmed),
        };
        let Some(safe_dir_part) = browser_completion_dir_part(dir_part) else {
            return Vec::new();
        };
        let dir = if safe_dir_part.as_os_str().is_empty() {
            self.root.clone()
        } else {
            self.root.join(&safe_dir_part)
        };
        if !dir.is_dir() {
            return Vec::new();
        }
        let display_dir_part = safe_dir_part.to_string_lossy().replace('\\', "/");

        let show_hidden = name_part.starts_with('.');
        let needle = name_part.to_lowercase();
        let mut entries = Vec::new();

        let mut builder = WalkBuilder::new(&dir);
        builder
            .hidden(!show_hidden)
            .follow_links(self.follow_links)
            .max_depth(Some(1));
        let _ = builder.add_custom_ignore_filename(".deepseekignore");

        for entry in builder.build().flatten() {
            let path = entry.path();
            if path == dir || path_is_excluded_from_discovery(&self.root, path) {
                continue;
            }
            let Some(file_type) = entry.file_type() else {
                continue;
            };
            if !file_type.is_file() && !file_type.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy();
            if !needle.is_empty() && !name.to_lowercase().starts_with(&needle) {
                continue;
            }
            let mut candidate = if display_dir_part.is_empty() {
                name.to_string()
            } else {
                format!("{display_dir_part}/{name}")
            };
            if file_type.is_dir() {
                candidate.push('/');
            }
            entries.push(candidate);
        }

        entries.sort_by_key(|entry| entry.to_lowercase());
        entries.truncate(limit);
        entries
    }
}

fn browser_completion_dir_part(dir_part: &str) -> Option<PathBuf> {
    let mut safe = PathBuf::new();
    for component in Path::new(dir_part).components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => safe.push(part),
            Component::Prefix(_) | Component::RootDir | Component::ParentDir => return None,
        }
    }
    Some(safe)
}

/// Default directory depth walked when surfacing file-mention completions.
/// Set high enough that conventionally nested source trees (Java/.NET/web
/// projects routinely reach 7-9 levels) stay reachable, while a `0` override
/// removes the limit entirely. Keeps Tab snappy in deep monorepos via the
/// `.gitignore`-aware walk and per-keypress candidate caps (#2488).
pub const DEFAULT_COMPLETIONS_WALK_DEPTH: usize = 10;

fn normalize_completion_walk_depth(depth: usize) -> Option<usize> {
    if depth == 0 { None } else { Some(depth) }
}

fn child_completion_walk_depth(depth: Option<usize>) -> Option<usize> {
    depth.map(|depth| depth.saturating_sub(1))
}

/// Hard cap on the number of `(file or directory)` entries indexed by
/// [`Workspace::build_file_index`]. The fuzzy-resolve index is a
/// convenience for [`Workspace::fuzzy_resolve`]; missing entries fall
/// back to literal-path resolution. Capping here keeps the first
/// `fuzzy_resolve` call bounded on huge workspaces (#697 reported a
/// ~10s hang on the first turn). For typical projects 50K is well
/// above the actual entry count and the cap is a no-op.
const FILE_INDEX_MAX_ENTRIES: usize = 50_000;

/// Configure a `WalkBuilder` for workspace discovery: hidden files,
/// depth-limited, custom `.deepseekignore` honored, and gitignore overrides
/// for AI-tool dot-directories so `@`-completion finds them even when
/// they're gitignored. Symlink following is controlled by `follow_links`.
fn discovery_walk_builder(
    root: &Path,
    max_depth: Option<usize>,
    follow_links: bool,
) -> WalkBuilder {
    let mut builder = WalkBuilder::new(root);
    builder.hidden(true).follow_links(follow_links);
    if let Some(depth) = max_depth {
        builder.max_depth(Some(depth));
    }
    let _ = builder.add_custom_ignore_filename(".deepseekignore");
    builder
}

/// Walk the AI-tool dot-directories (`.mimofan/`, `.cursor/`, `.claude/`,
/// `.agents/`) with gitignore disabled so their contents are discoverable
/// even when the project's `.gitignore` / `.ignore` excludes them.
fn walk_always_discoverable_dirs(
    walk_root: &Path,
    display_root: &Path,
    ctx: &mut SearchContext<'_>,
    max_depth: Option<usize>,
    follow_links: bool,
) {
    for dir_name in DISCOVERY_ALWAYS_DIRS {
        let dot_dir = walk_root.join(dir_name);
        if !dot_dir.is_dir() {
            continue;
        }
        let mut builder = WalkBuilder::new(&dot_dir);
        builder
            .hidden(true)
            .follow_links(follow_links)
            .git_ignore(false)
            .ignore(false);
        if let Some(depth) = max_depth {
            builder.max_depth(Some(depth.saturating_sub(1)));
        }
        for entry in builder.build().flatten() {
            if ctx.is_full() {
                break;
            }
            let path = entry.path();
            // Exclude machine-generated bulk (e.g. .mimofan/snapshots/)
            // even though gitignore is disabled for this walk.
            if path_is_excluded_from_discovery(walk_root, path) {
                continue;
            }
            let Ok(rel) = path.strip_prefix(display_root) else {
                continue;
            };
            let rel_str = rel.to_string_lossy().replace('\\', "/");
            if rel_str.is_empty() {
                continue;
            }
            let abs = path.to_path_buf();
            if !ctx.remember(abs) {
                continue;
            }
            let is_dir = entry.file_type().is_some_and(|ft| ft.is_dir());
            let candidate = if is_dir {
                format!("{rel_str}/")
            } else {
                rel_str.clone()
            };
            ctx.push_match(candidate);
        }
    }
}

fn walk_for_completions(
    walk_root: &Path,
    display_root: &Path,
    ctx: &mut SearchContext<'_>,
    max_depth: Option<usize>,
    follow_links: bool,
) {
    let builder = discovery_walk_builder(walk_root, max_depth, follow_links);

    for entry in builder.build().flatten() {
        if ctx.is_full() {
            break;
        }
        let path = entry.path();
        let Ok(rel) = path.strip_prefix(display_root) else {
            continue;
        };
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        if rel_str.is_empty() {
            continue;
        }
        // Dedup across the (cwd, workspace) double-walk by absolute path; we
        // want the cwd-relative display when both walks see the same file.
        let abs = path.to_path_buf();
        if !ctx.remember(abs) {
            continue;
        }
        let is_dir = entry.file_type().is_some_and(|ft| ft.is_dir());
        let candidate = if is_dir {
            format!("{rel_str}/")
        } else {
            rel_str.clone()
        };
        ctx.push_match(candidate);
    }

    // Also walk the AI-tool dot-directories with gitignore disabled so
    // `.mimofan/`, `.cursor/`, etc. are always discoverable.
    walk_always_discoverable_dirs(walk_root, display_root, ctx, max_depth, follow_links);
}

const LOCAL_REFERENCE_SCAN_LIMIT: usize = 4096;

fn add_local_reference_completions(
    root: &Path,
    display_root: &Path,
    ctx: &mut SearchContext<'_>,
    max_depth: Option<usize>,
    follow_links: bool,
) {
    if !should_try_local_reference_completion(ctx.needle) {
        return;
    }

    for path in local_reference_paths(root, LOCAL_REFERENCE_SCAN_LIMIT, max_depth, follow_links) {
        if ctx.is_full() {
            break;
        }
        let Ok(rel) = path.strip_prefix(display_root) else {
            continue;
        };
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        if rel_str.is_empty() || !ctx.remember(path.clone()) {
            continue;
        }
        ctx.push_match(rel_str);
    }
}

fn should_try_local_reference_completion(needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    // A bare separator or dot isn't an actionable path yet. Without this
    // guard, a single `@/` keystroke triggers a `LOCAL_REFERENCE_SCAN_LIMIT`
    // (4096-path) walk on the UI thread for #1921 — on WSL2 with a
    // `/mnt/c/...` workspace each entry crosses Windows-host I/O and the
    // composer appears frozen for seconds to minutes.
    if matches!(needle, "/" | "\\" | "." | "..") {
        return false;
    }
    needle.starts_with('.') || needle.contains('/') || needle.contains('\\')
}

fn local_reference_paths(
    root: &Path,
    limit: usize,
    max_depth: Option<usize>,
    follow_links: bool,
) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut builder = WalkBuilder::new(root);
    builder
        .hidden(false)
        .follow_links(follow_links)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false);
    if let Some(depth) = max_depth {
        builder.max_depth(Some(depth));
    }
    let _ = builder.add_custom_ignore_filename(".deepseekignore");
    let root_for_filter = root.to_path_buf();
    builder.filter_entry(move |entry| {
        !should_skip_unignored_discovery_entry(&root_for_filter, entry.path())
    });

    for entry in builder.build().flatten() {
        if out.len() >= limit {
            break;
        }
        let path = entry.path();
        if path == root {
            continue;
        }
        if entry
            .file_type()
            .is_some_and(|ft| ft.is_file() || ft.is_dir())
        {
            out.push(path.to_path_buf());
        }
    }
    out
}

impl Clone for Workspace {
    fn clone(&self) -> Self {
        // Don't carry the cached file_index — clones get a fresh OnceLock so
        // they don't pin a stale snapshot of the previous owner's tree.
        Self {
            root: self.root.clone(),
            cwd: self.cwd.clone(),
            file_index: OnceLock::new(),
            completion_walk_depth: self.completion_walk_depth,
            follow_links: self.follow_links,
        }
    }
}

fn expand_mention_home(path: &str) -> PathBuf {
    if path == "~"
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home);
    }
    if let Some(rest) = path.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home).join(rest);
    }
    PathBuf::from(path)
}

/// Configuration for working-set tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkingSetConfig {
    /// Maximum number of entries to keep.
    pub max_entries: usize,
    /// Maximum number of paths to pin during compaction.
    pub max_pinned_paths: usize,
    /// Maximum characters to scan per text block when pinning messages.
    pub max_scan_chars: usize,
    /// Maximum entries to show in the system prompt block.
    pub max_prompt_entries: usize,
}

impl Default for WorkingSetConfig {
    fn default() -> Self {
        Self {
            max_entries: 16,
            max_pinned_paths: 8,
            max_scan_chars: 2_000,
            max_prompt_entries: 8,
        }
    }
}

/// The source that most recently updated an entry.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum WorkingSetSource {
    UserMessage,
    ToolInput,
    ToolOutput,
    Rebuild,
}

/// A single working-set entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkingSetEntry {
    /// Workspace-relative path string.
    pub path: String,
    /// Whether the path is a directory (best-effort).
    pub is_dir: bool,
    /// Whether the path exists on disk (best-effort).
    pub exists: bool,
    /// Number of times this path was observed.
    pub touches: u32,
    /// The last observed turn index.
    pub last_turn: u64,
    /// The last update source.
    pub last_source: WorkingSetSource,
}

impl WorkingSetEntry {
    fn new(path: String, exists: bool, is_dir: bool, turn: u64, source: WorkingSetSource) -> Self {
        Self {
            path,
            is_dir,
            exists,
            touches: 1,
            last_turn: turn,
            last_source: source,
        }
    }
}

/// Repo-aware working-set state.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkingSet {
    /// Tracking configuration.
    pub config: WorkingSetConfig,
    /// Monotonic turn counter (increments on user messages).
    pub turn: u64,
    /// Path entries keyed by workspace-relative path.
    pub entries: HashMap<String, WorkingSetEntry>,
}

impl WorkingSet {
    /// Advance to the next turn.
    pub fn next_turn(&mut self) {
        self.turn = self.turn.saturating_add(1);
    }

    /// Observe a user message and update the working set.
    pub fn observe_user_message(&mut self, text: &str, workspace: &Path) {
        self.next_turn();
        let paths = extract_paths_from_text(text);
        self.record_candidates(paths, workspace, WorkingSetSource::UserMessage);
    }

    /// Observe a tool call (input and optional output).
    pub fn observe_tool_call(
        &mut self,
        tool_name: &str,
        input: &Value,
        output: Option<&str>,
        workspace: &Path,
    ) {
        let input_candidates = extract_paths_from_value(input, Some(tool_name));
        self.record_candidates(input_candidates, workspace, WorkingSetSource::ToolInput);

        if let Some(text) = output {
            let output_candidates = extract_paths_from_text(text);
            self.record_candidates(output_candidates, workspace, WorkingSetSource::ToolOutput);
        }
    }

    /// Rebuild the working set from existing messages (best effort).
    ///
    /// This is used when syncing a resumed session.
    pub fn rebuild_from_messages(&mut self, messages: &[Message], workspace: &Path) {
        self.entries.clear();
        self.turn = 0;

        for message in messages {
            if message.role == "user" {
                self.next_turn();
            }
            let candidates = extract_paths_from_message(message);
            if candidates.is_empty() {
                continue;
            }
            self.record_candidates(candidates, workspace, WorkingSetSource::Rebuild);
        }
    }

    /// Render a compact working-set block for the system prompt.
    ///
    /// Byte-stable across `next_turn()` calls when no new paths are observed
    /// (#280): the rendered lines drop the turn-relative `touches` and
    /// `last seen N turn(s) ago` fields, and the order is taken from
    /// `sorted_for_prompt` (turn-agnostic) instead of `sorted_entries`.
    /// The block lands in the system prompt before the historical
    /// conversation; any byte that drifts here cache-misses everything that
    /// follows in DeepSeek's KV prefix cache.
    pub fn summary_block(&self, workspace: &Path) -> Option<String> {
        let prompt_entries: Vec<&WorkingSetEntry> = self
            .sorted_for_prompt()
            .into_iter()
            .take(self.config.max_prompt_entries)
            .collect();

        let repo_summary = summarize_repo_root(workspace);

        if repo_summary.is_none() && prompt_entries.is_empty() {
            return None;
        }

        let mut lines: Vec<String> = Vec::new();
        lines.push("## Repo Working Set".to_string());
        lines.push(format!("Workspace: {}", workspace.display()));

        if let Some(summary) = repo_summary {
            lines.push(summary);
        }

        if !prompt_entries.is_empty() {
            lines.push("Active paths (prioritize these):".to_string());
            for entry in prompt_entries {
                let kind = if entry.is_dir { "dir" } else { "file" };
                lines.push(format!("- {} ({kind})", entry.path));
            }
        }

        lines.push(
            "When in doubt, use tools to verify and keep changes focused on the working set."
                .to_string(),
        );

        Some(lines.join("\n"))
    }

    /// Return the most relevant paths in score order.
    pub fn top_paths(&self, limit: usize) -> Vec<String> {
        self.sorted_entries()
            .into_iter()
            .take(limit)
            .map(|entry| entry.path.clone())
            .collect()
    }

    /// Identify message indices that should be pinned during compaction.
    pub fn pinned_message_indices(&self, messages: &[Message], workspace: &Path) -> Vec<usize> {
        if messages.is_empty() || self.entries.is_empty() {
            return Vec::new();
        }

        let pinned_paths: Vec<&WorkingSetEntry> = self
            .sorted_entries()
            .into_iter()
            .take(self.config.max_pinned_paths)
            .collect();
        if pinned_paths.is_empty() {
            return Vec::new();
        }

        let needles = build_search_needles(&pinned_paths, workspace);
        if needles.is_empty() {
            return Vec::new();
        }

        let mut pinned: Vec<usize> = Vec::new();
        for (idx, message) in messages.iter().enumerate() {
            if message_mentions_any_path(message, &needles, self.config.max_scan_chars) {
                pinned.push(idx);
            }
        }
        pinned
    }

    fn record_candidates(
        &mut self,
        candidates: Vec<String>,
        workspace: &Path,
        source: WorkingSetSource,
    ) {
        if candidates.is_empty() {
            return;
        }

        let workspace_canon = workspace.canonicalize().ok();

        for raw in candidates {
            let Some(normalized) = normalize_candidate(&raw) else {
                continue;
            };
            let Some((rel, exists, is_dir)) =
                relativize_candidate(&normalized, workspace, workspace_canon.as_deref())
            else {
                continue;
            };
            self.record_path(rel, exists, is_dir, source);
        }

        self.prune();
    }

    fn record_path(&mut self, rel: String, exists: bool, is_dir: bool, source: WorkingSetSource) {
        match self.entries.get_mut(&rel) {
            Some(entry) => {
                entry.exists |= exists;
                entry.is_dir |= is_dir;
                entry.touches = entry.touches.saturating_add(1);
                entry.last_turn = self.turn;
                entry.last_source = source;
            }
            None => {
                let entry = WorkingSetEntry::new(rel.clone(), exists, is_dir, self.turn, source);
                let _ = self.entries.insert(rel, entry);
            }
        }
    }

    fn prune(&mut self) {
        let max_entries = self.config.max_entries;
        if self.entries.len() <= max_entries {
            return;
        }

        // Rank by score ascending and drop the lowest until within bounds.
        let mut ranked: Vec<(String, i64)> = self
            .entries
            .values()
            .map(|entry| (entry.path.clone(), score_entry(entry, self.turn)))
            .collect();
        ranked.sort_by_key(|a| a.1);

        let to_remove = self.entries.len().saturating_sub(max_entries);
        for (path, _) in ranked.into_iter().take(to_remove) {
            let _ = self.entries.remove(&path);
        }
    }

    fn sorted_entries(&self) -> Vec<&WorkingSetEntry> {
        let mut entries: Vec<&WorkingSetEntry> = self.entries.values().collect();
        entries.sort_by(|a, b| {
            let sb = score_entry(b, self.turn);
            let sa = score_entry(a, self.turn);
            sb.cmp(&sa).then_with(|| a.path.cmp(&b.path))
        });
        entries
    }

    /// Turn-agnostic ordering used when rendering the prompt summary block.
    /// `sorted_entries` mixes in a recency bonus from `self.turn`, so its
    /// output reorders as turns advance even when no new paths are touched —
    /// that movement would cross `max_prompt_entries` boundaries and bust the
    /// KV prefix cache (#280). Compaction pinning still uses the recency-aware
    /// `sorted_entries`; only the prompt-facing surface is stabilised here.
    fn sorted_for_prompt(&self) -> Vec<&WorkingSetEntry> {
        let mut entries: Vec<&WorkingSetEntry> = self.entries.values().collect();
        entries.sort_by(|a, b| b.touches.cmp(&a.touches).then_with(|| a.path.cmp(&b.path)));
        entries
    }
}

fn score_entry(entry: &WorkingSetEntry, current_turn: u64) -> i64 {
    let age = current_turn.saturating_sub(entry.last_turn);
    let recency_bonus = match age {
        0 => 6,
        1 => 4,
        2 => 3,
        3..=5 => 2,
        6..=10 => 1,
        _ => 0,
    };
    i64::from(entry.touches) * 4 + recency_bonus
}

fn normalize_candidate(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_matches(|c: char| {
        matches!(
            c,
            '"' | '\'' | '`' | ',' | ';' | ':' | '(' | ')' | '[' | ']'
        )
    });
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_string())
}

fn relativize_candidate(
    candidate: &str,
    workspace: &Path,
    workspace_canon: Option<&Path>,
) -> Option<(String, bool, bool)> {
    let candidate_path = Path::new(candidate);

    // Reject obvious URLs and non-paths early.
    if candidate.contains("://") {
        return None;
    }

    let (rel_path, abs_path) = if candidate_path.is_absolute() {
        let within_workspace = workspace_canon
            .map(|ws| candidate_path.starts_with(ws))
            .unwrap_or_else(|| candidate_path.starts_with(workspace));
        if !within_workspace {
            return None;
        }
        let rel = candidate_path.strip_prefix(workspace).ok()?.to_path_buf();
        (rel, candidate_path.to_path_buf())
    } else {
        if starts_with_parent_dir(candidate_path) {
            return None;
        }
        let rel = clean_relative(candidate_path);
        let abs = workspace.join(&rel);
        (rel, abs)
    };

    let metadata = fs::metadata(&abs_path).ok();
    let exists = metadata.is_some();
    let is_dir = metadata
        .as_ref()
        .map(fs::Metadata::is_dir)
        .unwrap_or_else(|| candidate.ends_with('/'));

    let rel_string = path_to_string(&rel_path)?;
    Some((rel_string, exists, is_dir))
}

fn starts_with_parent_dir(path: &Path) -> bool {
    matches!(
        path.components().next(),
        Some(std::path::Component::ParentDir)
    )
}

fn clean_relative(path: &Path) -> PathBuf {
    use std::path::Component;

    let mut parts: Vec<PathBuf> = Vec::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                let _ = parts.pop();
            }
            Component::Normal(p) => parts.push(PathBuf::from(p)),
            Component::RootDir | Component::Prefix(_) => {}
        }
    }
    let mut out = PathBuf::new();
    for part in parts {
        out.push(part);
    }
    out
}

fn path_to_string(path: &Path) -> Option<String> {
    path.as_os_str().to_str().map(|s| s.replace('\\', "/"))
}

fn extract_paths_from_message(message: &Message) -> Vec<String> {
    let mut paths = Vec::new();
    for block in &message.content {
        match block {
            ContentBlock::Text { text, .. } => {
                paths.extend(extract_paths_from_text(text));
            }
            ContentBlock::ToolUse { input, .. } => {
                paths.extend(extract_paths_from_value(input, None));
            }
            ContentBlock::ToolResult { content, .. } => {
                paths.extend(extract_paths_from_text(content));
            }
            ContentBlock::Thinking { .. }
            | ContentBlock::ServerToolUse { .. }
            | ContentBlock::ToolSearchToolResult { .. }
            | ContentBlock::CodeExecutionToolResult { .. }
            | ContentBlock::ImageUrl { .. } => {}
        }
    }
    paths
}

fn extract_paths_from_value(value: &Value, tool_hint: Option<&str>) -> Vec<String> {
    let mut out = Vec::new();
    extract_paths_from_value_inner(value, tool_hint, None, &mut out);
    out
}

fn extract_paths_from_value_inner(
    value: &Value,
    tool_hint: Option<&str>,
    key_hint: Option<&str>,
    out: &mut Vec<String>,
) {
    match value {
        Value::String(s) => {
            let key_suggests_path = key_hint.map(key_is_path_like).unwrap_or(false);
            if key_suggests_path || looks_like_path(s) {
                out.extend(extract_paths_from_text(s));
                if key_suggests_path && !s.contains('/') && !s.contains('\\') {
                    out.push(s.to_string());
                }
            } else if tool_hint == Some("exec_shell") && s.len() < 400 {
                out.extend(extract_paths_from_text(s));
            }
        }
        Value::Array(arr) => {
            for item in arr {
                extract_paths_from_value_inner(item, tool_hint, key_hint, out);
            }
        }
        Value::Object(map) => {
            for (k, v) in map {
                extract_paths_from_value_inner(v, tool_hint, Some(k.as_str()), out);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn key_is_path_like(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    lower.contains("path")
        || lower.contains("file")
        || lower.contains("dir")
        || lower.contains("cwd")
        || lower.contains("workspace")
        || lower.contains("root")
        || lower == "target"
}

fn looks_like_path(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.contains('/') || trimmed.contains('\\') {
        return true;
    }
    match Path::new(trimmed).extension().and_then(OsStr::to_str) {
        Some(ext) => COMMON_EXTENSIONS.contains(&ext),
        None => false,
    }
}

const COMMON_EXTENSIONS: &[&str] = &[
    "rs", "toml", "md", "txt", "json", "yaml", "yml", "ts", "tsx", "js", "jsx", "py", "go", "java",
    "c", "cc", "cpp", "h", "hpp", "sh", "bash", "zsh", "sql", "html", "css", "scss",
];

fn extract_paths_from_text(text: &str) -> Vec<String> {
    if text.trim().is_empty() {
        return Vec::new();
    }

    let re = path_regex();
    re.find_iter(text)
        .map(|m| m.as_str().to_string())
        .filter(|s| looks_like_path(s))
        .collect()
}

fn path_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // Path-ish tokens with separators or file extensions.
        Regex::new(
            r#"(?x)
            (?:
                (?:[A-Za-z]:\\)?                # optional Windows drive
                (?:\./|\../|/)?                 # optional leading
                [A-Za-z0-9._-]+
                (?:[/\\][A-Za-z0-9._-]+)+
                (?:\.[A-Za-z0-9]{1,8})?         # optional extension
            )
            |
            (?:
                [A-Za-z0-9._-]+\.[A-Za-z0-9]{1,8}
            )
            "#,
        )
        .expect("path regex should compile")
    })
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

fn build_search_needles(entries: &[&WorkingSetEntry], workspace: &Path) -> Vec<String> {
    let mut needles: HashSet<String> = HashSet::new();
    for entry in entries {
        let rel = entry.path.clone();
        if rel.is_empty() {
            continue;
        }
        let abs = workspace.join(&rel);
        let abs_str = abs.as_os_str().to_str().map(ToOwned::to_owned);

        let _ = needles.insert(rel.clone());
        if let Some(abs_str) = abs_str {
            let _ = needles.insert(abs_str);
        }
    }
    needles.into_iter().collect()
}

fn message_mentions_any_path(message: &Message, needles: &[String], max_scan_chars: usize) -> bool {
    if needles.is_empty() {
        return false;
    }
    for block in &message.content {
        match block {
            ContentBlock::Text { text, .. } => {
                let snippet = truncate_chars(text, max_scan_chars);
                if contains_any(snippet, needles) {
                    return true;
                }
            }
            ContentBlock::ToolUse { input, .. } => {
                if let Ok(json) = serde_json::to_string(input)
                    && contains_any(&json, needles)
                {
                    return true;
                }
            }
            ContentBlock::ToolResult { content, .. } => {
                let snippet = truncate_chars(content, max_scan_chars);
                if contains_any(snippet, needles) {
                    return true;
                }
            }
            ContentBlock::Thinking { .. }
            | ContentBlock::ServerToolUse { .. }
            | ContentBlock::ToolSearchToolResult { .. }
            | ContentBlock::CodeExecutionToolResult { .. }
            | ContentBlock::ImageUrl { .. } => {}
        }
    }
    false
}

fn contains_any(text: &str, needles: &[String]) -> bool {
    needles
        .iter()
        .any(|needle| !needle.is_empty() && text.contains(needle))
}

fn summarize_repo_root(workspace: &Path) -> Option<String> {
    let key_files = detect_key_files(workspace);
    let top_dirs = list_top_level_dirs(workspace, 8);

    if key_files.is_empty() && top_dirs.is_empty() {
        return None;
    }

    let mut parts: Vec<String> = Vec::new();
    if !key_files.is_empty() {
        parts.push(format!("Key files: {}", key_files.join(", ")));
    }
    if !top_dirs.is_empty() {
        parts.push(format!("Top-level dirs: {}", top_dirs.join(", ")));
    }
    Some(parts.join("\n"))
}

fn detect_key_files(workspace: &Path) -> Vec<String> {
    const CANDIDATES: &[&str] = &[
        "Cargo.toml",
        "README.md",
        "AGENTS.md",
        "CLAUDE.md",
        "package.json",
        "pyproject.toml",
        "go.mod",
        "Makefile",
    ];

    CANDIDATES
        .iter()
        .filter_map(|name| {
            let path = workspace.join(name);
            if path.exists() {
                Some((*name).to_string())
            } else {
                None
            }
        })
        .collect()
}

fn list_top_level_dirs(workspace: &Path, limit: usize) -> Vec<String> {
    let mut dirs = Vec::new();
    let entries = match fs::read_dir(workspace) {
        Ok(entries) => entries,
        Err(_) => return dirs,
    };

    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            continue;
        };

        if name.starts_with('.') || IGNORED_ROOT_DIRS.contains(&name) {
            continue;
        }

        if let Ok(meta) = entry.metadata()
            && meta.is_dir()
        {
            dirs.push(name.to_string());
        }

        if dirs.len() >= limit {
            break;
        }
    }

    dirs.sort();
    dirs
}

const IGNORED_ROOT_DIRS: &[&str] = &["target", "node_modules", "dist", "build", ".git"];
