//! PR review command — fetches GitHub PR metadata and diff, then opens
//! the interactive TUI pre-seeded with a review prompt.

use super::*;

/// Open the interactive TUI pre-seeded with a GitHub PR's title, body,
/// and diff. Falls back gracefully if `gh` is missing.
pub(crate) async fn run_pr(
    cli: &Cli,
    config: &Config,
    number: u32,
    repo: Option<&str>,
    checkout: bool,
) -> Result<()> {
    if !is_command_available("gh") {
        bail!(
            "`gh` CLI not found on PATH. Install GitHub CLI \
             (https://cli.github.com) and authenticate (`gh auth login`) \
             so `mimofan pr <N>` can fetch PR metadata and the diff."
        );
    }

    let view = run_gh_pr_view(number, repo)?;
    let diff = run_gh_pr_diff(number, repo)?;

    if checkout {
        match run_gh_pr_checkout(number, repo) {
            Ok(()) => eprintln!("Checked out PR #{number} into the current workspace."),
            Err(err) => eprintln!(
                "warning: gh pr checkout #{number} failed ({err}). Continuing without checkout."
            ),
        }
    }

    let prompt = format_pr_prompt(number, &view, &diff);
    let resume_session_id = if cli.continue_session {
        let workspace = crate::resolve_workspace(cli);
        crate::latest_session_id_for_workspace(&workspace)
            .ok()
            .flatten()
    } else {
        cli.resume.clone()
    };
    crate::run_interactive(
        cli,
        config,
        resume_session_id,
        Some(crate::tui::InitialInput::Prefill(prompt)),
    )
    .await
}

/// Return true if `name` resolves to an executable on the current `PATH`.
///
/// Walks `$PATH` directly instead of probing with `--version`. The
/// previous implementation invoked `Command::new(name).arg("--version")`,
/// which fails on the Ubuntu CI runner because `/bin/sh` is `dash` —
/// `dash --version` exits with status 2 ("invalid option") even though
/// `sh` is plainly on PATH. macOS happens to ship bash as `sh`, which
/// does honor `--version`, so the bug was invisible locally and only
/// surfaced in CI logs.
///
/// Windows: also checks the `.exe` extension when `name` doesn't have
/// one, matching the platform's PATHEXT lookup behavior for the common
/// case.
pub(crate) fn is_command_available(name: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return true;
        }
    }
    false
}

#[derive(Debug, Clone, Default)]
pub(crate) struct GhPullRequest {
    pub(crate) title: String,
    pub(crate) body: String,
    pub(crate) base: String,
    pub(crate) head: String,
    pub(crate) url: String,
}

pub(crate) fn run_gh_pr_view(number: u32, repo: Option<&str>) -> Result<GhPullRequest> {
    let mut cmd = crate::dependencies::Gh::command()
        .ok_or_else(|| anyhow::anyhow!("gh not found on PATH"))?;
    cmd.arg("pr").arg("view").arg(number.to_string());
    if let Some(r) = repo {
        cmd.arg("--repo").arg(r);
    }
    cmd.arg("--json")
        .arg("title,body,baseRefName,headRefName,url");
    let output = cmd
        .output()
        .map_err(|e| anyhow::anyhow!("Failed to run `gh pr view`: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        bail!("gh pr view #{number} failed: {stderr}");
    }
    let raw = String::from_utf8_lossy(&output.stdout).to_string();
    let value: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|e| anyhow::anyhow!("gh pr view returned non-JSON output: {e}"))?;
    let pick = |key: &str| {
        value
            .get(key)
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    Ok(GhPullRequest {
        title: pick("title"),
        body: pick("body"),
        base: pick("baseRefName"),
        head: pick("headRefName"),
        url: pick("url"),
    })
}

pub(crate) fn run_gh_pr_diff(number: u32, repo: Option<&str>) -> Result<String> {
    let mut cmd = crate::dependencies::Gh::command()
        .ok_or_else(|| anyhow::anyhow!("gh not found on PATH"))?;
    cmd.arg("pr").arg("diff").arg(number.to_string());
    if let Some(r) = repo {
        cmd.arg("--repo").arg(r);
    }
    let output = cmd
        .output()
        .map_err(|e| anyhow::anyhow!("Failed to run `gh pr diff`: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        bail!("gh pr diff #{number} failed: {stderr}");
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

pub(crate) fn run_gh_pr_checkout(number: u32, repo: Option<&str>) -> Result<()> {
    let mut cmd = crate::dependencies::Gh::command()
        .ok_or_else(|| anyhow::anyhow!("gh not found on PATH"))?;
    cmd.arg("pr").arg("checkout").arg(number.to_string());
    if let Some(r) = repo {
        cmd.arg("--repo").arg(r);
    }
    let output = cmd
        .output()
        .map_err(|e| anyhow::anyhow!("Failed to run `gh pr checkout`: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        bail!("gh pr checkout #{number} failed: {stderr}");
    }
    Ok(())
}

/// Format the PR review prompt that lands in the composer. Caps the
/// diff at 200 KiB so a massive PR doesn't blow the model's context
/// window before the user even hits Enter — they can always ask the
/// model to fetch more via `gh pr diff #N` from inside the session.
pub(crate) fn format_pr_prompt(number: u32, view: &GhPullRequest, diff: &str) -> String {
    const MAX_DIFF_BYTES: usize = 200 * 1024;
    let diff_section = if diff.len() > MAX_DIFF_BYTES {
        let cut = (0..=MAX_DIFF_BYTES)
            .rev()
            .find(|&i| diff.is_char_boundary(i))
            .unwrap_or(0);
        format!(
            "{}\n\n[…diff truncated at {} KiB; ask me to fetch more if needed]\n",
            &diff[..cut],
            MAX_DIFF_BYTES / 1024
        )
    } else {
        diff.to_string()
    };
    let body = if view.body.trim().is_empty() {
        "(no description)".to_string()
    } else {
        view.body.trim().to_string()
    };
    let title = if view.title.trim().is_empty() {
        format!("(PR #{number})")
    } else {
        view.title.trim().to_string()
    };
    let branches = match (view.base.is_empty(), view.head.is_empty()) {
        (false, false) => format!("{} ← {}", view.base, view.head),
        (false, true) => view.base.clone(),
        (true, false) => view.head.clone(),
        _ => "(unknown)".to_string(),
    };
    format!(
        "Review PR #{number} — {title}\n\
         \n\
         URL: {url}\n\
         Branches: {branches}\n\
         \n\
         ## Description\n\
         \n\
         {body}\n\
         \n\
         ## Diff\n\
         \n\
         ```diff\n\
         {diff_section}\n\
         ```\n",
        url = if view.url.is_empty() {
            "(unavailable)"
        } else {
            view.url.as_str()
        },
    )
}
