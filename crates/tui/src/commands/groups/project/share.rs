//! /share command — export the current session.
//!
//! By default the session transcript is rendered as static HTML and uploaded
//! to a GitHub Gist via the `gh` CLI, printing the resulting URL.
//!
//! With `/share --local` the session is written to a local Markdown file
//! instead, so it can be exported without a `gh` CLI or any network upload.
//!
//! # Usage
//!
//! - `/share` — export the current session and print the Gist URL
//! - `/share --local` — write the session to a local `.md` file
//! - `/share help` — show usage

use std::io::Write;
use std::path::{Path, PathBuf};

use super::CommandResult;
use crate::dependencies::ExternalTool;
use crate::tui::app::{App, AppAction};

/// Share the current session as a web URL (or a local file with `--local`).
pub fn share(app: &mut App, arg: Option<&str>) -> CommandResult {
    let raw = arg.map(str::trim).unwrap_or("");

    match raw {
        "" => do_share(app, false),
        "--local" | "-l" => do_share(app, true),
        "help" | "--help" | "-h" => CommandResult::message(
            "/share — Export the current session.\n\
             \n\
             Usage:\n\
             /share           Export and upload the current session (GitHub Gist URL)\n\
             /share --local   Write the session to a local Markdown file (no upload)\n\
             /share help      Show this help\n\
             \n\
             The default mode renders the session as static HTML and uploads it\n\
             to a GitHub Gist via the `gh` CLI. `--local` writes a `.md` file to\n\
             the current directory instead, so no `gh` CLI or network is required."
                .to_string(),
        ),
        _ => CommandResult::error(format!(
            "Unknown /share argument `{raw}`. Use `/share`, `/share --local`, or `/share help`."
        )),
    }
}

/// Export the session (as HTML+Gist, or as a local file) and show the result.
fn do_share(app: &mut App, local: bool) -> CommandResult {
    // Check if there's any session content to share
    if app.history.is_empty() {
        return CommandResult::error("Nothing to share. The current session is empty.");
    }

    // Sanity-check: the extra info block is optional; the session itself
    // is what we share.
    let history_len = app.history.len();
    let model = &app.model;
    let mode = app.mode.label();

    // Use an AppAction to signal the engine to perform the async work.
    let hint = if local {
        format!("Exporting {history_len} cell(s) from {model} ({mode}) session to a local file...")
    } else {
        format!(
            "Exporting {history_len} cell(s) from {model} ({mode}) session...\n\n\
             The session will be rendered as static HTML and uploaded to a GitHub Gist.\n\
             This requires the `gh` CLI to be installed and authenticated."
        )
    };
    CommandResult::with_message_and_action(
        hint,
        AppAction::ShareSession {
            history_len,
            model: model.clone(),
            mode: mode.to_string(),
            local,
        },
    )
}

/// Actually perform the share export.
///
/// This is called from the engine after receiving the `ShareSession` action.
/// When `local` is true the session is written to a local Markdown file and
/// its path is returned; otherwise it is rendered as HTML and uploaded via
/// `gh gist create`, returning the Gist URL.
pub async fn perform_share(
    history_json: &str,
    model: &str,
    mode: &str,
    local: bool,
) -> Result<String, String> {
    if local {
        let md = render_session_markdown(history_json, model, mode);
        let path = write_local_markdown(&md)?;
        return Ok(path.to_string_lossy().to_string());
    }

    // Build HTML from the session data
    let html = render_session_html(history_json, model, mode);

    // Write to a temp file
    let tmp = match write_temp_html(&html) {
        Ok(file) => file,
        Err(e) => return Err(format!("Failed to write temp file: {e}")),
    };

    // Upload via `gh gist create`
    let url = match upload_gist(tmp.path()).await {
        Ok(url) => url,
        Err(e) => return Err(format!("Failed to upload Gist: {e}")),
    };

    Ok(url)
}

/// Render the session as a standalone HTML page.
fn render_session_html(history_json: &str, model: &str, mode: &str) -> String {
    let timestamp = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC");
    let escaped_model = html_escape(model);
    let escaped_mode = html_escape(mode);
    let escaped_body = html_escape(history_json);

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>mimofan Session Export</title>
<style>
  body {{
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
    max-width: 800px; margin: 2rem auto; padding: 0 1rem;
    background: #0d1117; color: #c9d1d9;
  }}
  h1 {{ color: #58a6ff; border-bottom: 1px solid #30363d; padding-bottom: 0.5rem; }}
  .meta {{ color: #8b949e; font-size: 0.9rem; margin-bottom: 2rem; }}
  .message {{ margin: 1rem 0; padding: 0.75rem; border-radius: 6px; }}
  .user {{ background: #1f2937; border-left: 3px solid #58a6ff; }}
  .assistant {{ background: #161b22; border-left: 3px solid #3fb950; }}
  .tool {{ background: #0d1117; border: 1px solid #30363d; font-family: monospace; font-size: 0.85rem; }}
  pre {{ white-space: pre-wrap; word-wrap: break-word; margin: 0; }}
  .footer {{ margin-top: 2rem; padding-top: 1rem; border-top: 1px solid #30363d; color: #8b949e; font-size: 0.8rem; }}
</style>
</head>
<body>
<h1>mimofan Session</h1>
<div class="meta">
  <strong>Model:</strong> {escaped_model} · <strong>Mode:</strong> {escaped_mode}<br>
  <strong>Exported:</strong> {timestamp}
</div>
<pre>{escaped_body}</pre>
<div class="footer">
  Generated by mimofan · https://github.com/XiaomingX/mimofan
</div>
</body>
</html>"#,
    )
}

/// HTML-escape special characters.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Render the session as a standalone Markdown document (used by `/share --local`).
fn render_session_markdown(history_json: &str, model: &str, mode: &str) -> String {
    let timestamp = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC");
    let mut out = String::new();
    out.push_str("# mimofan Session\n\n");
    out.push_str(&format!("**Model:** {model} · **Mode:** {mode}  \n"));
    out.push_str(&format!("**Exported:** {timestamp}\n\n"));
    out.push_str("---\n\n");
    out.push_str("```json\n");
    out.push_str(history_json);
    out.push_str("\n```\n");
    out
}

/// Write the Markdown export to a local file named after the current timestamp.
fn write_local_markdown(md: &str) -> Result<PathBuf, String> {
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let filename = format!("mimofan-session-{stamp}.md");
    let path = std::env::current_dir()
        .map_err(|e| format!("Cannot resolve current directory: {e}"))?
        .join(&filename);
    let mut file =
        std::fs::File::create(&path).map_err(|e| format!("Failed to create {filename}: {e}"))?;
    file.write_all(md.as_bytes())
        .map_err(|e| format!("Failed to write {filename}: {e}"))?;
    Ok(path)
}

/// Write HTML to a secure temp file and keep it alive for upload.
fn write_temp_html(html: &str) -> Result<tempfile::NamedTempFile, String> {
    let mut tmp = tempfile::Builder::new()
        .prefix("mimofan-share-")
        .suffix(".html")
        .tempfile()
        .map_err(|e| format!("{e}"))?;
    tmp.write_all(html.as_bytes()).map_err(|e| format!("{e}"))?;
    Ok(tmp)
}

/// Upload a file as a GitHub Gist using the `gh` CLI.
async fn upload_gist(path: &Path) -> Result<String, String> {
    let path_owned = path.to_path_buf();
    let output = tokio::task::spawn_blocking(move || {
        let mut cmd = crate::dependencies::Gh::command()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "gh not found"))?;
        cmd.args([
            "gist",
            "create",
            "--public",
            path_owned.to_string_lossy().as_ref(),
            "--filename",
            "session-export.html",
            "--desc",
            "mimofan Session Export",
        ])
        .output()
    })
    .await
    .map_err(|join_err| format!("gh gist create panicked: {join_err}"))?
    .map_err(|e| format!("Failed to run `gh gist create`: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("`gh gist create` failed: {stderr}"));
    }

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if stdout.is_empty() {
        return Err("`gh gist create` returned no output".to_string());
    }

    Ok(stdout)
}
