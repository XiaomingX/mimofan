//! Configuration merging helpers extracted from `lib.rs`.

use super::*;

pub(crate) fn merge_project_config(config: &mut Config, workspace: &Path) {
    // When the workspace is the user's home directory, the project-scope
    // config file is also the global config file. Skip the merge to avoid
    // redundant processing and a misleading "project-scope config key
    // ignored" warning on every launch from ~.
    if let Some(home) = effective_home_dir()
        && let (Ok(w), Ok(h)) = (
            std::fs::canonicalize(workspace),
            std::fs::canonicalize(&home),
        )
        && w == h
    {
        return;
    }

    let path = workspace
        .join(mimofan_config::MIMOFAN_APP_DIR)
        .join("config.toml");
    let raw = match read_project_config_file(&path) {
        Ok(Some(r)) => r,
        Ok(None) => return,
        Err(err) => {
            eprintln!(
                "warning: failed to read project-scope config {}: {err}",
                path.display()
            );
            return;
        }
    };
    let project: toml::Value = match toml::from_str(&raw) {
        Ok(v) => v,
        Err(_) => return,
    };
    let table = match project.as_table() {
        Some(t) => t,
        None => return,
    };

    // #417: dangerous keys are denied at project scope. A malicious
    // `<workspace>/.mimofan/config.toml` could otherwise:
    // * `api_key` / `base_url` / `provider` — exfiltrate prompts to a
    //   look-alike endpoint by swapping the user's credentials and
    //   target host with project-controlled values.
    // * `mcp_config_path` — point the loader at an MCP config that
    //   spawns arbitrary stdio servers under the user's identity.
    // * `mcp_oauth_callback_*` — choose local OAuth redirect listener
    //   behavior for user-owned MCP credentials.
    //
    // The overlay path is non-interactive; users can't visually
    // confirm a rogue project config is hijacking these. We surface
    // a stderr warning on first encounter so a user who *did* expect
    // the override has a chance to notice the deny instead of silent
    // discard.
    const DENY_AT_PROJECT_SCOPE: &[&str] = &[
        "api_key",
        "base_url",
        "provider",
        "mcp_config_path",
        "mcp_oauth_callback_port",
        "mcp_oauth_callback_url",
    ];
    for key in DENY_AT_PROJECT_SCOPE {
        if table.contains_key(*key) {
            eprintln!(
                "warning: project-scope config key `{key}` is ignored — \
                 set it in `~/.mimofan/config.toml` instead. \
                 (See #417 for the deny-list rationale.)"
            );
        }
    }

    // String fields a project may legitimately override (model,
    // approval/sandbox tightening, notes path, reasoning effort).
    for (key, field) in [
        ("model", &mut config.default_text_model),
        ("reasoning_effort", &mut config.reasoning_effort),
        ("notes_path", &mut config.notes_path),
    ] {
        if let Some(v) = table.get(key).and_then(toml::Value::as_str)
            && !v.is_empty()
        {
            *field = Some(v.to_string());
        }
    }

    if let Some(v) = table.get("approval_policy").and_then(toml::Value::as_str)
        && !v.is_empty()
    {
        if mimofan_config::project_approval_policy_is_allowed(config.approval_policy.as_deref(), v)
        {
            config.approval_policy = Some(v.to_string());
        } else {
            eprintln!(
                "warning: project-scope `approval_policy = \"{v}\"` is ignored — \
                 project config can only tighten the user's approval policy. \
                 (See #417.)"
            );
        }
    }

    if let Some(v) = table.get("sandbox_mode").and_then(toml::Value::as_str)
        && !v.is_empty()
    {
        if mimofan_config::project_sandbox_mode_is_allowed(config.sandbox_mode.as_deref(), v) {
            config.sandbox_mode = Some(v.to_string());
        } else {
            eprintln!(
                "warning: project-scope `sandbox_mode = \"{v}\"` is ignored — \
                 project config can only tighten the user's sandbox mode. \
                 (See #417.)"
            );
        }
    }

    // Numeric / bool fields that benefit from per-project overrides.
    if let Some(v) = table.get("max_subagents").and_then(toml::Value::as_integer)
        && v > 0
    {
        config.max_subagents = Some((v as usize).clamp(1, crate::config::MAX_SUBAGENTS));
    }
    if let Some(v) = table.get("allow_shell").and_then(toml::Value::as_bool) {
        if v {
            eprintln!(
                "warning: project-scope `allow_shell = true` is ignored — \
                 enable shell from user config for this workspace instead. \
                 (See #417.)"
            );
        } else {
            config.allow_shell = Some(false);
        }
    }

    if table.contains_key("instructions") {
        eprintln!(
            "warning: project-scope `instructions` is ignored — \
             configure instruction files from user config instead. \
             (See #417.)"
        );
    }
}

pub(crate) fn read_project_config_file(path: &Path) -> io::Result<Option<String>> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err),
    };
    let file_type = metadata.file_type();
    if file_type.is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "project-scope config must not be a symlink",
        ));
    }
    if !file_type.is_file() {
        return Ok(None);
    }

    let mut file = open_project_config_file(path)?;
    let mut raw = String::new();
    file.read_to_string(&mut raw)?;
    Ok(Some(raw))
}

#[cfg(unix)]
pub(crate) fn open_project_config_file(path: &Path) -> io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;

    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
}

#[cfg(not(unix))]
pub(crate) fn open_project_config_file(path: &Path) -> io::Result<std::fs::File> {
    std::fs::File::open(path)
}

pub(crate) fn merge_user_workspace_config(
    config: &mut Config,
    config_path: Option<PathBuf>,
    workspace: &Path,
) {
    if config.managed_config_path.is_some() || config.requirements_path.is_some() {
        return;
    }
    let allow_shell_before = config.allow_shell;
    let allow_shell_from_env = std::env::var_os("MIMOFAN_ALLOW_SHELL").is_some();
    let Some(path) = crate::config::resolve_load_config_path(config_path) else {
        return;
    };
    let Ok(raw) = std::fs::read_to_string(path) else {
        return;
    };
    let Ok(doc) = toml::from_str::<toml::Value>(&raw) else {
        return;
    };
    merge_user_workspace_config_from_doc(config, &doc, workspace);
    if allow_shell_from_env {
        config.allow_shell = allow_shell_before;
    }
}

pub(crate) fn merge_user_workspace_config_from_doc(
    config: &mut Config,
    doc: &toml::Value,
    workspace: &Path,
) {
    for table_name in ["workspace", "projects"] {
        let Some(entries) = doc.get(table_name).and_then(toml::Value::as_table) else {
            continue;
        };
        for (raw_path, entry) in entries {
            if !workspace_config_path_matches(raw_path, workspace) {
                continue;
            }
            if let Some(allow_shell) = entry.get("allow_shell").and_then(toml::Value::as_bool) {
                config.allow_shell = Some(allow_shell);
            }
        }
    }
}

pub(crate) fn workspace_config_path_matches(raw_path: &str, workspace: &Path) -> bool {
    let configured = crate::config::expand_path(raw_path);
    let configured = configured.canonicalize().unwrap_or(configured);
    let workspace = workspace
        .canonicalize()
        .unwrap_or_else(|_| workspace.to_path_buf());
    paths_equal_for_config(&configured, &workspace)
}

#[cfg(not(windows))]
pub(crate) fn paths_equal_for_config(left: &Path, right: &Path) -> bool {
    left == right
}

#[cfg(any(windows, test))]
pub(crate) fn normalize_windows_config_path_str(path: &str) -> String {
    let mut normalized = path.replace('/', "\\");
    if let Some(rest) = normalized.strip_prefix(r"\\?\UNC\") {
        normalized = format!("\\\\{rest}");
    } else if let Some(rest) = normalized.strip_prefix(r"\\?\") {
        normalized = rest.to_string();
    }
    while normalized.len() > 3 && normalized.ends_with('\\') {
        normalized.pop();
    }
    normalized.to_ascii_lowercase()
}
