//! Filesystem path resolution helpers for config/cache/workspace locations.
//!
//! Pure path-building helpers extracted verbatim from `config.rs`. They depend
//! only on `std`, `dirs`, and `shellexpand` plus one another, so they form a
//! clean leaf. `config.rs` pulls them back in (`use paths::{...}`) for the
//! workspace-trust and config-loading logic that stays there, and re-exports
//! the two `pub(crate)` entry points (`effective_home_dir`, `expand_path`) so
//! external `crate::config::` callers resolve unchanged (#3311).
//!
//! Visibility note: helpers that were file-private `fn` in `config.rs` are
//! `pub(crate)` here purely so the parent module can name them; none are
//! re-exported publicly, so the crate's external surface is unchanged.

use std::path::{Path, PathBuf};

pub(crate) fn default_config_path() -> Option<PathBuf> {
    env_config_path().or_else(home_config_path)
}

pub(crate) fn mimofan_home_dir() -> Option<PathBuf> {
    mimofan_config::mimofan_home().ok()
}

pub(crate) fn effective_home_dir() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("HOME") {
        let path = PathBuf::from(path);
        if !path.as_os_str().is_empty() {
            return Some(path);
        }
    }

    if let Some(path) = std::env::var_os("USERPROFILE") {
        let path = PathBuf::from(path);
        if !path.as_os_str().is_empty() {
            return Some(path);
        }
    }

    dirs::home_dir()
}

pub(crate) fn home_config_path() -> Option<PathBuf> {
    mimofan_config::resolve_config_path(None).ok()
}

pub(crate) fn workspace_config_key(workspace: &Path) -> String {
    canonicalize_or_keep(workspace)
        .to_string_lossy()
        .into_owned()
}

pub(crate) fn canonicalize_or_keep(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

pub(crate) fn env_config_path() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("MIMOFAN_CONFIG_PATH") {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            return Some(expand_path(trimmed));
        }
    }
    None
}

pub(crate) fn expand_pathbuf(path: PathBuf) -> PathBuf {
    if let Some(raw) = path.to_str() {
        return expand_path(raw);
    }
    path
}

pub(crate) fn default_managed_config_path() -> Option<PathBuf> {
    #[cfg(unix)]
    {
        Some(PathBuf::from("/etc/mimofan/managed_config.toml"))
    }
    #[cfg(not(unix))]
    {
        mimofan_config::resolve_state_dir("managed_config.toml").ok()
    }
}

pub(crate) fn default_requirements_path() -> Option<PathBuf> {
    #[cfg(unix)]
    {
        Some(PathBuf::from("/etc/mimofan/requirements.toml"))
    }
    #[cfg(not(unix))]
    {
        mimofan_config::resolve_state_dir("requirements.toml").ok()
    }
}

pub(crate) fn expand_path(path: &str) -> PathBuf {
    if let Some(stripped) = path.strip_prefix('~')
        && (stripped.is_empty() || stripped.starts_with('/') || stripped.starts_with('\\'))
        && let Some(mut home) = effective_home_dir()
    {
        let suffix = stripped.trim_start_matches(['/', '\\']);
        if !suffix.is_empty() {
            home.push(suffix);
        }
        return home;
    }

    let expanded = shellexpand::tilde(path);
    PathBuf::from(expanded.as_ref())
}

pub(crate) fn default_skills_dir() -> Option<PathBuf> {
    mimofan_config::resolve_state_dir("skills").ok()
}

pub(crate) fn default_mcp_config_path() -> Option<PathBuf> {
    #[cfg(unix)]
    {
        Some(PathBuf::from("/etc/mimofan/mcp.json"))
    }
    #[cfg(not(unix))]
    {
        mimofan_config::resolve_state_dir("mcp.json").ok()
    }
}

pub(crate) fn default_notes_path() -> Option<PathBuf> {
    mimofan_config::resolve_state_dir("notes.txt").ok()
}

pub(crate) fn default_memory_path() -> Option<PathBuf> {
    mimofan_config::resolve_state_dir("memory.md").ok()
}

pub(crate) fn default_memory_dir() -> Option<PathBuf> {
    mimofan_config::resolve_state_dir("memory").ok()
}
