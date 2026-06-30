//! Local media attachment commands.

use std::path::{Path, PathBuf};

use super::CommandResult;
use crate::tui::app::App;

pub fn attach(app: &mut App, arg: Option<&str>) -> CommandResult {
    let Some(raw_path) = arg.map(str::trim).filter(|value| !value.is_empty()) else {
        return CommandResult::error("Usage: /attach <image-or-video-path>");
    };

    let path = resolve_attachment_path(raw_path, &app.workspace);
    let Ok(path) = path.canonicalize() else {
        return CommandResult::error(format!("Attachment not found: {}", path.display()));
    };
    if !path.is_file() {
        return CommandResult::error(format!("Attachment is not a file: {}", path.display()));
    }

    let Some(kind) = media_kind(&path) else {
        return CommandResult::error(
            "Unsupported attachment type. /attach is for image/video paths; use @path for text files or directories.",
        );
    };

    app.insert_media_attachment(kind, &path, None);
    CommandResult::message(format!("Attached {kind}: {}", path.display()))
}

fn resolve_attachment_path(raw_path: &str, workspace: &Path) -> PathBuf {
    let unquoted = raw_path.trim().trim_matches('"').trim_matches('\'');
    let path = expand_home(unquoted);
    if path.is_absolute() {
        path
    } else {
        workspace.join(path)
    }
}

fn expand_home(path: &str) -> PathBuf {
    if path == "~" {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home);
        }
    } else if let Some(rest) = path.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home).join(rest);
    }
    PathBuf::from(path)
}

fn media_kind(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tif" | "tiff" | "ppm" => Some("image"),
        "mp4" | "mov" | "m4v" | "webm" | "avi" | "mkv" => Some("video"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {}
