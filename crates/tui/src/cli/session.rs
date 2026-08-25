//! Session management helpers extracted from `lib.rs`.

use super::*;

pub(crate) fn sessions_resume_command() -> &'static str {
    "mimofan resume"
}

pub(crate) fn list_sessions(limit: usize, search: Option<String>) -> Result<()> {
    use crate::palette;
    use colored::Colorize;
    use session_manager::{SessionManager, SessionSearchHit, format_session_line};

    let (accent_r, accent_g, accent_b) = palette::MIMOFAN_ACCENT_PRIMARY_RGB;
    let (sky_r, sky_g, sky_b) = palette::MIMOFAN_SKY_RGB;
    let (aqua_r, aqua_g, aqua_b) = palette::MIMOFAN_SKY_RGB;

    let manager = SessionManager::default_location()?;

    let sessions: Vec<SessionSearchHit> = if let Some(query) = search {
        manager
            .search_sessions_fulltext(&query)?
            .into_iter()
            .collect()
    } else {
        manager
            .list_sessions()?
            .into_iter()
            .map(SessionSearchHit::from)
            .collect()
    };

    if sessions.is_empty() {
        println!("{}", "No sessions found.".truecolor(sky_r, sky_g, sky_b));
        println!(
            "Start a new session with: {}",
            "mimofan".truecolor(accent_r, accent_g, accent_b)
        );
        return Ok(());
    }

    println!(
        "{}",
        "Saved Sessions"
            .truecolor(accent_r, accent_g, accent_b)
            .bold()
    );
    println!("{}", "==============".truecolor(sky_r, sky_g, sky_b));
    println!();

    for (i, hit) in sessions.iter().take(limit).enumerate() {
        let line = format_session_line(&hit.metadata);
        if i == 0 {
            println!("  {} {}", "*".truecolor(aqua_r, aqua_g, aqua_b), line);
        } else {
            println!("    {line}");
        }
        if let Some(snippet) = &hit.snippet {
            println!(
                "      {} {}",
                "↳".truecolor(sky_r, sky_g, sky_b),
                snippet.truecolor(sky_r, sky_g, sky_b)
            );
        }
    }

    let total = sessions.len();
    if total > limit {
        println!();
        println!(
            "  {} more session(s). Use --limit to show more.",
            total - limit
        );
    }

    println!();
    println!(
        "Resume with: {} {}",
        sessions_resume_command().truecolor(accent_r, accent_g, accent_b),
        "<session-id>".dimmed()
    );
    println!(
        "Continue latest in this workspace: {}",
        "mimofan --continue".truecolor(accent_r, accent_g, accent_b)
    );

    Ok(())
}

pub(crate) fn resolve_workspace(cli: &Cli) -> PathBuf {
    cli.workspace
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

pub(crate) fn load_config_from_cli(cli: &Cli) -> Result<Config> {
    let profile = cli
        .profile
        .clone()
        .or_else(|| std::env::var("MIMOFAN_PROFILE").ok());
    let mut config = Config::load(cli.config.clone(), profile.as_deref())?;
    cli.feature_toggles.apply(&mut config)?;
    Ok(config)
}

pub(crate) fn read_api_key_from_stdin() -> Result<String> {
    let mut stdin = io::stdin();
    if stdin.is_terminal() {
        bail!("No API key provided. Pass --api-key or pipe one via stdin.");
    }
    let mut buffer = String::new();
    stdin.read_to_string(&mut buffer)?;
    let api_key = buffer.trim().to_string();
    if api_key.is_empty() {
        bail!("No API key provided via stdin.");
    }
    Ok(api_key)
}

pub(crate) fn run_login(api_key: Option<String>) -> Result<()> {
    let api_key = match api_key {
        Some(key) => key,
        None => read_api_key_from_stdin()?,
    };
    let saved = config::save_api_key(&api_key)?;
    println!("Saved API key to {}", saved.describe());
    Ok(())
}

pub(crate) fn run_logout() -> Result<()> {
    config::clear_api_key()?;
    println!("Cleared saved API key.");
    Ok(())
}

pub(crate) fn resolve_session_id(
    session_id: Option<String>,
    last: bool,
    workspace: &Path,
) -> Result<String> {
    if last {
        return latest_session_id_for_workspace(workspace)?.ok_or_else(|| {
            anyhow!(
                "No saved sessions found for workspace {}. Use `mimofan sessions` to list all sessions, or `mimofan resume <SESSION_ID>` to resume one explicitly.",
                workspace.display()
            )
        });
    }
    if let Some(id) = session_id {
        return Ok(id);
    }
    pick_session_id()
}

pub(crate) fn latest_session_id_for_workspace(workspace: &Path) -> std::io::Result<Option<String>> {
    let manager = SessionManager::default_location()?;
    Ok(manager
        .get_latest_session_for_workspace(workspace)?
        .map(|session| session.id))
}

pub(crate) fn fork_session(
    session_id: Option<String>,
    last: bool,
    workspace: &Path,
) -> Result<String> {
    let manager = SessionManager::default_location()?;
    let saved = if last {
        let Some(meta) = manager.get_latest_session_for_workspace(workspace)? else {
            bail!(
                "No saved sessions found for workspace {}.",
                workspace.display()
            );
        };
        manager.load_session(&meta.id)?
    } else {
        let id = resolve_session_id(session_id, false, workspace)?;
        manager.load_session_by_prefix(&id)?
    };

    let system_prompt = saved
        .system_prompt
        .as_ref()
        .map(|text| SystemPrompt::Text(text.clone()));
    let mut forked = create_saved_session(
        &saved.messages,
        &saved.metadata.model,
        &saved.metadata.workspace,
        saved.metadata.total_tokens,
        system_prompt.as_ref(),
    );
    forked.metadata.copy_cost_from(&saved.metadata);
    forked.metadata.mark_forked_from(&saved.metadata);
    manager.save_session(&forked)?;

    let source_title = saved.metadata.title.trim();
    let source_label = if source_title.is_empty() {
        "session".to_string()
    } else {
        format!("\"{source_title}\"")
    };
    println!(
        "Forked {source_label} ({source_id}) → new session {new_id}",
        source_id = truncate_id(&saved.metadata.id),
        new_id = truncate_id(&forked.metadata.id),
    );

    Ok(forked.metadata.id)
}

pub(crate) fn pick_session_id() -> Result<String> {
    let manager = SessionManager::default_location()?;
    let sessions = manager.list_sessions()?;
    if sessions.is_empty() {
        bail!("No saved sessions found.");
    }

    println!("Select a session to resume:");
    for (idx, session) in sessions.iter().enumerate() {
        println!("  {:>2}. {} ({})", idx + 1, session.title, session.id);
    }
    print!("Enter a number (or press Enter to cancel): ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let input = input.trim();
    if input.is_empty() {
        bail!("No session selected.");
    }
    let idx: usize = input
        .parse()
        .map_err(|_| anyhow::anyhow!("Invalid input"))?;
    let session = sessions
        .get(idx.saturating_sub(1))
        .ok_or_else(|| anyhow::anyhow!("Selection out of range"))?;
    Ok(session.id.clone())
}

pub(crate) fn load_recent_checkpoint(
    manager: &session_manager::SessionManager,
) -> Option<(session_manager::SavedSession, std::time::Duration)> {
    let session = manager.load_checkpoint().ok().flatten()?;

    let checkpoint_path = manager
        .sessions_dir()
        .join("checkpoints")
        .join("latest.json");
    let metadata = std::fs::metadata(&checkpoint_path).ok()?;
    let mtime = metadata.modified().ok()?;
    let age = std::time::SystemTime::now().duration_since(mtime).ok()?;
    if age > std::time::Duration::from_secs(24 * 3600) {
        let _ = manager.clear_checkpoint();
        return None;
    }

    Some((session, age))
}

pub(crate) fn checkpoint_age_label(age: std::time::Duration) -> String {
    if age.as_secs() < 60 {
        format!("{}s ago", age.as_secs())
    } else if age.as_secs() < 3600 {
        format!("{}m ago", age.as_secs() / 60)
    } else {
        format!("{}h ago", age.as_secs() / 3600)
    }
}

/// Check for a crash-recovery checkpoint and return the session ID if explicit
/// recovery was requested *and* the checkpoint belongs to the current
/// workspace.
///
/// The checkpoint must exist and its file mtime must be within 24 hours.
/// **The checkpoint's workspace must also match the resolved launch workspace
/// after canonicalisation.** If the workspace doesn't match, the checkpoint is
/// persisted as a regular session (so the user can find it via
/// `mimofan sessions` / `mimofan resume <id>`) and cleared, but not loaded.
pub(crate) fn recover_interrupted_checkpoint_for_resume(launch_workspace: &Path) -> Option<String> {
    let manager = session_manager::SessionManager::default_location().ok()?;
    let (session, age) = load_recent_checkpoint(&manager)?;

    // Refuse to silently restore a session from another workspace. Compare
    // against the resolved launch workspace, not the shell cwd, so callers
    // using `--workspace` cannot accidentally recover a checkpoint from the
    // directory their shell happened to be in.
    let session_workspace = session.metadata.workspace.clone();
    let workspace_matches =
        session_manager::workspace_scope_matches(&session_workspace, launch_workspace);

    if !workspace_matches {
        // Persist the checkpoint so the user can find it via `mimofan
        // sessions`, then clear it so the next launch in this folder doesn't
        // re-trip the nag. Print a one-line notice pointing at the explicit
        // resume command — but DO NOT auto-load the session here.
        let _ = manager.save_session(&session);
        let _ = manager.clear_checkpoint();
        eprintln!(
            "Note: an interrupted session from another workspace ({}) is \
             available. Run `mimofan sessions` to list saved sessions. Starting \
             fresh in {}.",
            session_workspace.display(),
            launch_workspace.display(),
        );
        return None;
    }

    let session_id = session.metadata.id.clone();

    // Persist the checkpoint as a regular session so the TUI can load it by id.
    if manager.save_session(&session).is_err() {
        return None;
    }

    // Clear the checkpoint now that it has been recovered.
    let _ = manager.clear_checkpoint();

    let age_str = checkpoint_age_label(age);
    eprintln!("Recovered interrupted session ({age_str}). Use --fresh to start fresh.",);

    Some(session_id)
}

/// Preserve an interrupted checkpoint on a normal fresh launch without
/// attaching it to the new TUI instance. This keeps "open another mimofan in
/// the same folder" from re-entering the previous in-flight session while still
/// leaving an explicit resume path.
pub(crate) fn preserve_interrupted_checkpoint_for_explicit_resume(launch_workspace: &Path) {
    let Some(manager) = session_manager::SessionManager::default_location().ok() else {
        return;
    };
    let Some((session, age)) = load_recent_checkpoint(&manager) else {
        return;
    };

    let session_workspace = session.metadata.workspace.clone();
    let _ = manager.save_session(&session);
    let _ = manager.clear_checkpoint();

    let age_str = checkpoint_age_label(age);
    if session_manager::workspace_scope_matches(&session_workspace, launch_workspace) {
        eprintln!(
            "Found an in-flight session snapshot ({age_str}). Starting a new \
             session. Run `mimofan --continue` to resume it."
        );
    } else {
        eprintln!(
            "Note: an interrupted session from another workspace ({}) is \
             available. Run `mimofan sessions` to list saved sessions. Starting \
             fresh in {}.",
            session_workspace.display(),
            launch_workspace.display(),
        );
    }
}
