//! Skill discovery and registry for local SKILL.md files.

pub mod install;
mod system;
// Re-exports kept for documentation parity and downstream consumers; the
// binary itself imports directly from `skills::install`. `#[allow(...)]`
// silences the dead-code warning that fires because no `bin` source path
// references these names through `skills::*`.
#[allow(unused_imports)]
pub use install::{
    DEFAULT_MAX_SIZE_BYTES, DEFAULT_REGISTRY_URL, INSTALLED_FROM_MARKER, InstallOutcome,
    InstallSource, InstalledSkill, RegistryDocument, RegistryEntry, RegistryFetchResult,
    SkillSyncOutcome, SyncResult, UpdateResult, default_cache_skills_dir,
};
pub use system::{install_system_skills, is_bundled_skill_name};

use std::fs;
use std::path::{Path, PathBuf};

use std::collections::{HashMap, HashSet};

use crate::logging;

const MAX_SKILL_DESCRIPTION_CHARS: usize = 280;
const MAX_AVAILABLE_SKILLS_CHARS: usize = 12_000;

// === Defaults ===

#[must_use]
pub fn default_skills_dir() -> PathBuf {
    dirs::home_dir().map_or_else(
        || PathBuf::from("/tmp/mimofan/skills"),
        |p| p.join(".mimofan").join("skills"),
    )
}

/// Global agentskills.io-compatible skills directory (`~/.agents/skills`).
#[must_use]
pub fn agents_global_skills_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|p| p.join(".agents").join("skills"))
}

// === Types ===

/// Session-time skill discovery scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillDiscoveryMode {
    /// Preserve the existing broad compatibility scan across mimofan,
    /// agentskills.io, Claude, OpenCode, Cursor, and legacy DeepSeek roots.
    Compatible,
    /// Scan only mimofan-owned roots. Callers that also pass an explicit
    /// `skills_dir` still get that directory because it is user configuration.
    mimofanOnly,
}

impl SkillDiscoveryMode {
    #[must_use]
    pub fn from_mimofan_only(value: bool) -> Self {
        if value {
            Self::mimofanOnly
        } else {
            Self::Compatible
        }
    }
}

/// Parsed representation of a SKILL.md definition.
#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub body: String,
    /// On-disk path to the `SKILL.md` this was loaded from. The directory
    /// name can differ from the frontmatter `name` for community installs
    /// or manually-placed skills, so callers must use this rather than
    /// reconstructing `<dir>/<name>/SKILL.md`.
    pub path: PathBuf,
}

/// Collection of discovered skills.
#[derive(Debug, Clone, Default)]
pub struct SkillRegistry {
    skills: Vec<Skill>,
    warnings: Vec<String>,
}

impl SkillRegistry {
    /// Maximum directory-traversal depth when discovering skills.
    ///
    /// Defends against pathological configurations (e.g. a user pointing
    /// `skills_dir` at `~`) without artificially limiting realistic
    /// vendored layouts like `<root>/<org>/<repo>/<skill>/SKILL.md`.
    const MAX_DISCOVERY_DEPTH: usize = 8;

    /// Discover skills from the given directory.
    ///
    /// The search walks `dir` recursively: any directory that contains a
    /// `SKILL.md` is loaded as a single skill, and the walk does **not**
    /// descend further into that directory (companion files live next to
    /// `SKILL.md`, and `tools::skill::collect_companion_files` already
    /// treats nested subdirs as out-of-scope). This lets users organize
    /// skills by vendor / category — e.g.
    /// `<root>/<vendor>/<skill>/SKILL.md` — instead of being forced into
    /// a flat `<root>/<skill>/SKILL.md` layout.
    ///
    /// Hidden subdirectories (names starting with `.`) below the root
    /// are skipped to avoid descending into VCS / cache trees like
    /// `.git/`. The provided `dir` itself is always honored, even if
    /// hidden — that's what the user explicitly configured.
    /// Symlinked directories are followed when they resolve to directories,
    /// with canonical path tracking plus [`Self::MAX_DISCOVERY_DEPTH`] keeping
    /// the walk finite when a skills layout contains cycles.
    #[must_use]
    pub fn discover(dir: &Path) -> Self {
        let mut registry = Self::default();
        let Ok(canonical_dir) = fs::canonicalize(dir) else {
            return registry;
        };
        if !canonical_dir.is_dir() {
            return registry;
        }

        let mut visited = HashSet::new();
        Self::discover_recursive(dir, 0, &mut registry, &mut visited);
        registry
            .skills
            .sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.path.cmp(&b.path)));
        registry
    }

    fn discover_recursive(
        dir: &Path,
        depth: usize,
        registry: &mut Self,
        visited: &mut HashSet<PathBuf>,
    ) {
        if depth > Self::MAX_DISCOVERY_DEPTH {
            return;
        }
        if !Self::mark_discovered_dir(dir, visited) {
            return;
        }

        let entries = match fs::read_dir(dir) {
            Ok(e) => e,
            Err(err) => {
                // Only surface a warning for the user-provided root
                // (depth == 0). Nested permission errors are usually
                // noise (e.g. a stray `.Trash` inside someone's
                // `~/.agents/skills`).
                if depth == 0 {
                    registry.push_warning(format!(
                        "Failed to read skills directory {}: {err}",
                        dir.display()
                    ));
                }
                return;
            }
        };

        for entry in entries.flatten() {
            let path = entry.path();
            // Skip hidden subdirectories. Common offenders are `.git`,
            // `.cache`, `.Trash`. The provided root itself is exempt:
            // the user explicitly pointed `skills_dir` at it and we
            // never filter it (it's passed directly to this function,
            // not iterated). This check applies to *children* of the
            // current directory at every depth — including depth 0,
            // because a `.git/` right next to the skills we want is
            // exactly the kind of noise we must not descend into.
            if path
                .file_name()
                .and_then(|s| s.to_str())
                .is_some_and(|name| name.starts_with('.'))
            {
                continue;
            }

            let Ok(metadata) = fs::metadata(&path) else {
                continue;
            };
            if !metadata.is_dir() {
                continue;
            }

            let skill_path = path.join("SKILL.md");
            match fs::read_to_string(&skill_path) {
                Ok(content) => match Self::parse_skill(&skill_path, &content) {
                    Ok(mut skill) => {
                        if !Self::mark_discovered_dir(&path, visited) {
                            continue;
                        }
                        skill.path = skill_path.clone();
                        registry.skills.push(skill);
                        // This directory IS a skill. Don't descend further:
                        // any nested `SKILL.md` would be a fixture or
                        // example bundled with the parent skill, not a
                        // separately-installable skill.
                        continue;
                    }
                    Err(reason) => {
                        if !Self::mark_discovered_dir(&path, visited) {
                            continue;
                        }
                        registry.push_warning(format!(
                            "Failed to parse {}: {reason}",
                            skill_path.display()
                        ));
                        // Still treat this directory as "claimed" — a
                        // malformed SKILL.md shouldn't cause us to
                        // double-load nested fixtures as skills.
                        continue;
                    }
                },
                Err(err) if skill_path.exists() => {
                    if !Self::mark_discovered_dir(&path, visited) {
                        continue;
                    }
                    registry
                        .push_warning(format!("Failed to read {}: {err}", skill_path.display()));
                    continue;
                }
                Err(_) => {
                    // No SKILL.md here — recurse to look for nested
                    // skill directories (e.g. `<vendor>/<skill>/SKILL.md`).
                }
            }

            Self::discover_recursive(&path, depth + 1, registry, visited);
        }
    }

    fn mark_discovered_dir(dir: &Path, visited: &mut HashSet<PathBuf>) -> bool {
        let key = fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
        visited.insert(key)
    }

    fn push_warning(&mut self, warning: String) {
        logging::warn(&warning);
        self.warnings.push(warning);
    }

    fn parse_skill(_path: &Path, content: &str) -> std::result::Result<Skill, String> {
        let trimmed = content.trim_start();

        // Try to parse frontmatter block first. If absent, fall back to
        // extracting the first `# Heading` as the skill name so that plain
        // Markdown files (no `---` fence) are accepted instead of rejected.
        if trimmed.starts_with("---") {
            let start = content
                .find("---")
                .ok_or_else(|| "missing frontmatter opening delimiter".to_string())?;
            let rest = &content[start + 3..];
            let end = rest
                .find("---")
                .ok_or_else(|| "missing frontmatter closing delimiter".to_string())?;
            let frontmatter = &rest[..end];
            let body = &rest[end + 3..];

            let mut metadata = HashMap::new();
            let lines: Vec<&str> = frontmatter.lines().collect();
            let mut i = 0;
            while i < lines.len() {
                let raw = lines[i];
                let line = raw.trim();
                if line.is_empty() || line.starts_with('#') {
                    i += 1;
                    continue;
                }
                if let Some((key, value)) = line.split_once(':') {
                    let value = value.trim();
                    // Check for YAML block scalar indicators: > (folded), | (literal),
                    // optionally with chomping: >-, >+, |-, |+
                    let is_block_scalar = matches!(value, ">" | "|" | ">-" | ">+" | "|-" | "|+");
                    if is_block_scalar {
                        let is_folded = value.starts_with('>');
                        let chomp = if value.ends_with('-') {
                            "strip"
                        } else if value.ends_with('+') {
                            "keep"
                        } else {
                            "clip"
                        };
                        // Determine the base indentation from the key line
                        let base_indent = raw.len() - raw.trim_start().len();
                        let mut block_lines: Vec<&str> = Vec::new();
                        let mut content_indent: Option<usize> = None;
                        i += 1;
                        while i < lines.len() {
                            let raw_line = lines[i];
                            if raw_line.trim().is_empty() {
                                // Empty lines are part of the block
                                block_lines.push("");
                                i += 1;
                                continue;
                            }
                            let line_indent = raw_line.len() - raw_line.trim_start().len();
                            if line_indent > base_indent {
                                // Track content indent from the first non-empty
                                // line so we strip only that one level of
                                // leading whitespace, preserving any deeper
                                // relative indentation (YAML §8.1.2).
                                if content_indent.is_none() {
                                    content_indent = Some(line_indent);
                                }
                                block_lines.push(raw_line);
                                i += 1;
                            } else {
                                break;
                            }
                        }
                        let content_indent = content_indent.unwrap_or(base_indent);
                        // Strip only the content indent from each non-empty
                        // line so nested indentation survives.
                        let block_lines: Vec<&str> = block_lines
                            .iter()
                            .map(|raw| {
                                if raw.is_empty() {
                                    ""
                                } else {
                                    let indent = raw.len() - raw.trim_start().len();
                                    let strip = std::cmp::min(indent, content_indent);
                                    &raw[strip..]
                                }
                            })
                            .collect();
                        // Apply chomping to trailing empty lines before folding.
                        // Chomping operates on the raw block_lines (before join), so
                        // strip / keep / clip behave per the YAML spec.
                        let block_lines = if matches!(chomp, "strip") {
                            // strip: remove all trailing empty lines
                            let mut lines = block_lines;
                            while lines.last().is_some_and(|s| s.is_empty()) {
                                lines.pop();
                            }
                            lines
                        } else if matches!(chomp, "keep") {
                            // keep: no modification
                            block_lines
                        } else {
                            // clip: keep at most one trailing empty line
                            let mut lines = block_lines;
                            while lines.len() >= 2
                                && lines[lines.len() - 1].is_empty()
                                && lines[lines.len() - 2].is_empty()
                            {
                                lines.pop();
                            }
                            lines
                        };
                        let description = if is_folded {
                            // Folded: join non-empty lines with spaces; empty
                            // lines become paragraph breaks.
                            let mut result = String::new();
                            let mut pending_space = false;
                            for line in &block_lines {
                                if line.is_empty() {
                                    result.push('\n');
                                    pending_space = false;
                                } else {
                                    if pending_space {
                                        result.push(' ');
                                    }
                                    result.push_str(line);
                                    pending_space = true;
                                }
                            }
                            result
                        } else {
                            // Literal: join with newlines.
                            block_lines.join("\n")
                        };
                        metadata.insert(key.trim().to_ascii_lowercase(), description);
                    } else {
                        let unquoted = match value {
                            v if (v.starts_with('"') && v.ends_with('"') && v.len() >= 2)
                                || (v.starts_with('\'') && v.ends_with('\'') && v.len() >= 2) =>
                            {
                                &v[1..v.len() - 1]
                            }
                            _ => value,
                        };
                        metadata.insert(key.trim().to_ascii_lowercase(), unquoted.to_string());
                        i += 1;
                    }
                } else {
                    i += 1;
                }
            }

            let name = metadata
                .get("name")
                .filter(|name| !name.is_empty())
                .cloned()
                .ok_or_else(|| "missing required frontmatter field: name".to_string())?;

            let description = metadata.get("description").cloned().unwrap_or_default();

            return Ok(Skill {
                name,
                description,
                body: body.trim().to_string(),
                // Filled in by `discover` after parse succeeds; default to an
                // empty path so direct constructors (e.g. tests) compile.
                path: PathBuf::new(),
            });
        }

        // Graceful degradation: no frontmatter fence found.
        // Extract the first `# Heading` as the skill name.
        let heading_re = regex::Regex::new(r"(?m)^#\s+(.+)$").expect("static regex is valid");
        let name = heading_re
            .captures(content)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().trim().to_string())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                "no frontmatter and no `# Heading` found to use as skill name".to_string()
            })?;

        Ok(Skill {
            name,
            description: String::new(),
            body: content.trim().to_string(),
            path: PathBuf::new(),
        })
    }

    /// Lookup a skill by name.
    pub fn get(&self, name: &str) -> Option<&Skill> {
        self.skills.iter().find(|s| s.name == name)
    }

    /// Return all loaded skills.
    pub fn list(&self) -> &[Skill] {
        &self.skills
    }

    /// Parse or I/O warnings encountered while discovering skills.
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// Check whether any skills were loaded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }

    /// Return the number of loaded skills.
    #[must_use]
    pub fn len(&self) -> usize {
        self.skills.len()
    }
}

/// Resolve every candidate skills directory for a workspace, in
/// precedence order — most specific first. Used for session-time
/// skill discovery so the model sees skills that originated in
/// other AI-tool conventions installed in the same workspace
/// (#432).
///
/// Precedence (first match wins on name conflicts):
///
/// 1. `<workspace>/.agents/skills` — deepseek-native convention.
/// 2. `<workspace>/skills` — flat, project-local.
/// 3. `<workspace>/.opencode/skills` — OpenCode interop.
/// 4. `<workspace>/.claude/skills` — Claude Code interop.
/// 5. `<workspace>/.cursor/skills` — Cursor interop.
/// 6. `<workspace>/.mimofan/skills` — mimofan workspace skills.
/// 7. [`agents_global_skills_dir`] — agentskills.io global.
/// 8. `~/.claude/skills` — Claude-ecosystem global (#902).
/// 9. `~/.mimofan/skills` — mimofan global, primary install target.
/// 10. `~/.deepseek/skills` — legacy DeepSeek global fallback.
///
/// Only directories that exist on disk are returned — callers don't
/// need to filter further. Returns an empty vec when nothing is
/// installed (the system-prompt skills block is then suppressed).
#[must_use]
#[allow(dead_code)]
pub fn skills_directories(workspace: &Path) -> Vec<PathBuf> {
    skills_directories_for_mode(workspace, SkillDiscoveryMode::Compatible)
}

#[must_use]
pub fn skills_directories_for_mode(workspace: &Path, mode: SkillDiscoveryMode) -> Vec<PathBuf> {
    let home = dirs::home_dir();
    skills_directories_with_home_and_mode(workspace, home.as_deref(), mode)
}

fn skills_directories_with_home_and_mode(
    workspace: &Path,
    home_dir: Option<&Path>,
    mode: SkillDiscoveryMode,
) -> Vec<PathBuf> {
    let mut candidates = match mode {
        SkillDiscoveryMode::Compatible => vec![
            workspace.join(".agents").join("skills"),
            workspace.join("skills"),
            workspace.join(".opencode").join("skills"),
            workspace.join(".claude").join("skills"),
            workspace.join(".cursor").join("skills"),
            workspace.join(".mimofan").join("skills"),
        ],
        SkillDiscoveryMode::mimofanOnly => mimofan_workspace_skills_dir(workspace)
            .into_iter()
            .collect(),
    };
    if let Some(home) = home_dir {
        match mode {
            SkillDiscoveryMode::Compatible => {
                candidates.push(home.join(".agents").join("skills"));
                candidates.push(home.join(".claude").join("skills"));
                candidates.push(home.join(".mimofan").join("skills"));
                candidates.push(home.join(".deepseek").join("skills"));
            }
            SkillDiscoveryMode::mimofanOnly => {
                candidates.push(home.join(".mimofan").join("skills"));
            }
        }
    } else {
        candidates.push(PathBuf::from("/tmp/mimofan/skills"));
    }
    existing_skill_dirs(candidates)
}

pub(crate) fn mimofan_workspace_skills_dir(workspace: &Path) -> Option<PathBuf> {
    let skills_dir = workspace.join(".mimofan").join("skills");
    let canonical_workspace = fs::canonicalize(workspace).ok()?;
    let canonical_skills = fs::canonicalize(&skills_dir).ok()?;
    (canonical_skills.is_dir() && canonical_skills.starts_with(canonical_workspace))
        .then_some(skills_dir)
}

fn existing_skill_dirs(candidates: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for path in candidates {
        let Ok(canonical_path) = fs::canonicalize(&path) else {
            continue;
        };
        if canonical_path.is_dir() && seen.insert(canonical_path) {
            out.push(path);
        }
    }
    out
}

/// Walk every candidate skills directory for a workspace and merge
/// the discovered skills into a single registry. Name conflicts are
/// resolved with first-match-wins precedence per
/// [`skills_directories`].
///
/// Warnings from each scanned directory accumulate so the model
/// (and the user via `/skill list`) can see why a skill didn't
/// load.
#[must_use]
pub fn discover_in_workspace(workspace: &Path) -> SkillRegistry {
    discover_in_workspace_with_mode(workspace, SkillDiscoveryMode::Compatible)
}

#[must_use]
pub fn discover_in_workspace_with_mode(
    workspace: &Path,
    mode: SkillDiscoveryMode,
) -> SkillRegistry {
    let mut merged = SkillRegistry::default();
    for dir in skills_directories_for_mode(workspace, mode) {
        let registry = SkillRegistry::discover(&dir);
        for skill in registry.skills {
            if !merged.skills.iter().any(|s| s.name == skill.name) {
                merged.skills.push(skill);
            }
        }
        for warning in registry.warnings {
            merged.warnings.push(warning);
        }
    }
    merged
}

/// Discover skills from the workspace search set plus the configured install
/// directory. Workspace-local directories keep their normal precedence; a
/// custom configured directory is inserted before global defaults when it is
/// outside that set so explicit configuration cannot be buried by large global
/// libraries.
#[must_use]
#[allow(dead_code)]
pub fn discover_for_workspace_and_dir(workspace: &Path, skills_dir: &Path) -> SkillRegistry {
    discover_for_workspace_and_dir_with_mode(workspace, skills_dir, SkillDiscoveryMode::Compatible)
}

#[must_use]
pub fn discover_for_workspace_and_dir_with_mode(
    workspace: &Path,
    skills_dir: &Path,
    mode: SkillDiscoveryMode,
) -> SkillRegistry {
    let dirs = skill_directories_for_workspace_and_dir(workspace, skills_dir, mode);
    discover_from_directories(dirs)
}

#[must_use]
pub fn skill_directories_for_workspace_and_dir(
    workspace: &Path,
    skills_dir: &Path,
    mode: SkillDiscoveryMode,
) -> Vec<PathBuf> {
    let mut dirs = skills_directories_for_mode(workspace, mode);
    insert_configured_skills_dir(&mut dirs, workspace, skills_dir);
    dirs
}

fn insert_configured_skills_dir(dirs: &mut Vec<PathBuf>, workspace: &Path, skills_dir: &Path) {
    if !skills_dir.is_dir() || dirs.iter().any(|p| paths_refer_to_same_dir(p, skills_dir)) {
        return;
    }

    let workspace_root = fs::canonicalize(workspace).ok();
    let insert_at = workspace_root
        .as_ref()
        .and_then(|root| {
            dirs.iter()
                .position(|dir| fs::canonicalize(dir).map_or(true, |dir| !dir.starts_with(root)))
        })
        .unwrap_or(dirs.len());
    dirs.insert(insert_at, skills_dir.to_path_buf());
}

fn paths_refer_to_same_dir(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    match (fs::canonicalize(left), fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

pub(crate) fn discover_from_directories(dirs: impl IntoIterator<Item = PathBuf>) -> SkillRegistry {
    let mut merged = SkillRegistry::default();
    for dir in dirs {
        let registry = SkillRegistry::discover(&dir);
        for skill in registry.skills {
            if !merged.skills.iter().any(|s| s.name == skill.name) {
                merged.skills.push(skill);
            }
        }
        for warning in registry.warnings {
            merged.warnings.push(warning);
        }
    }
    merged
}

/// Render the system-prompt skills block from every workspace
/// candidate directory plus the global default (#432). Wraps
/// [`discover_in_workspace`] for callers (e.g. `prompts.rs`) that
/// only have the workspace path to hand.
#[must_use]
pub fn render_available_skills_context_for_workspace(workspace: &Path) -> Option<String> {
    let registry = discover_in_workspace(workspace);
    render_skills_block(&registry)
}

#[must_use]
pub fn render_available_skills_context_for_workspace_with_mode(
    workspace: &Path,
    mode: SkillDiscoveryMode,
) -> Option<String> {
    let registry = discover_in_workspace_with_mode(workspace, mode);
    render_skills_block(&registry)
}

/// Codex's progressive-disclosure contract: the model sees skill names,
/// descriptions, and paths up front, then opens the specific `SKILL.md` only
/// when a skill is relevant.
///
/// Single-directory variant — use
/// [`render_available_skills_context_for_workspace`] when scanning
/// a workspace for cross-tool skill folders (#432).
#[cfg(test)]
#[must_use]
fn render_available_skills_context(skills_dir: &Path) -> Option<String> {
    let registry = SkillRegistry::discover(skills_dir);
    render_skills_block(&registry)
}

/// Union variant: merge skills discovered in the `workspace` (cross-tool skill
/// folders) and an explicitly-configured `skills_dir`.
#[must_use]
pub fn render_available_skills_context_for_workspace_and_dir(
    workspace: &Path,
    skills_dir: &Path,
) -> Option<String> {
    render_available_skills_context_for_workspace_and_dir_with_mode(
        workspace,
        skills_dir,
        SkillDiscoveryMode::Compatible,
    )
}

#[must_use]
pub fn render_available_skills_context_for_workspace_and_dir_with_mode(
    workspace: &Path,
    skills_dir: &Path,
    mode: SkillDiscoveryMode,
) -> Option<String> {
    let registry = discover_for_workspace_and_dir_with_mode(workspace, skills_dir, mode);
    render_skills_block(&registry)
}

fn render_skills_block(registry: &SkillRegistry) -> Option<String> {
    if registry.is_empty() {
        return None;
    }

    let mut out = String::new();
    out.push_str("## Skills\n");
    out.push_str(
        "A skill is a set of local instructions stored in a `SKILL.md` file. \
Below is the list of skills available in this session. Each entry includes a \
name, description, and file path so you can open the source for full \
instructions when using a specific skill.\n\n",
    );
    out.push_str("### Available skills\n");

    let mut omitted = 0usize;
    for skill in registry.list() {
        // Use the real on-disk path captured at discovery — the directory
        // name can differ from the frontmatter `name` for community
        // installs, in which case `<dir>/<name>/SKILL.md` would not exist
        // and the model would fail to open it.
        let description = truncate_for_prompt(&skill.description, MAX_SKILL_DESCRIPTION_CHARS);
        let line = if description.is_empty() {
            format!("- {}: (file: {})\n", skill.name, skill.path.display())
        } else {
            format!(
                "- {}: {} (file: {})\n",
                skill.name,
                description,
                skill.path.display()
            )
        };

        if out.chars().count() + line.chars().count() > MAX_AVAILABLE_SKILLS_CHARS {
            omitted += 1;
        } else {
            out.push_str(&line);
        }
    }

    if omitted > 0 {
        out.push_str(&format!(
            "- ... {omitted} additional skills omitted from this prompt budget.\n"
        ));
    }

    if !registry.warnings().is_empty() {
        out.push_str("\n### Skill load warnings\n");
        for warning in registry.warnings().iter().take(8) {
            out.push_str("- ");
            out.push_str(&truncate_for_prompt(warning, MAX_SKILL_DESCRIPTION_CHARS));
            out.push('\n');
        }
    }

    out.push_str(
        "\n### How to use skills\n\
- Skill bodies live on disk at the listed paths. When a skill is relevant, open only that skill's `SKILL.md` and the specific companion files it references.\n\
- Trigger rules: use a skill when the user names it (`$SkillName`, `/skill <name>`, or plain text) or the task clearly matches its description. Do not carry skills across turns unless re-mentioned.\n\
- Missing/blocked: if a named skill is missing or cannot be read, say so briefly and continue with the best fallback.\n\
- Safety: do not execute scripts from a community skill unless the user explicitly asks or the skill has been trusted for script use.\n",
    );

    Some(out)
}

fn truncate_for_prompt(value: &str, max_chars: usize) -> String {
    let single_line = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if single_line.chars().count() <= max_chars {
        return single_line;
    }

    let mut truncated = single_line
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    truncated.push('…');
    truncated
}

#[cfg(test)]
mod tests {}
