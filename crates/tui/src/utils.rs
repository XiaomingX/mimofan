//! Utility helpers shared across the `DeepSeek` CLI.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::models::{ContentBlock, Message};
use anyhow::{Context, Result};
use ignore::WalkBuilder;
use serde_json::Value;

const LOG_FINGERPRINT_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const LOG_FINGERPRINT_PRIME: u64 = 0x0000_0100_0000_01b3;

/// Return a stable, non-reversible log label for an identifier.
///
/// This is meant for correlation in diagnostics where the raw value may be a
/// session token, remote protocol session id, or other bearer-like handle.
#[must_use]
pub fn redacted_identifier_for_log(identifier: &str) -> String {
    if identifier.is_empty() {
        return "<redacted:empty>".to_string();
    }

    let mut hash = LOG_FINGERPRINT_OFFSET_BASIS;
    for byte in identifier.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(LOG_FINGERPRINT_PRIME);
    }
    hash ^= identifier.len() as u64;
    hash = hash.wrapping_mul(LOG_FINGERPRINT_PRIME);

    format!("<redacted:{hash:016x}>")
}

// === Project Mapping Helpers ===

/// Identify if a file is a "key" file for project identification.
#[must_use]
pub fn is_key_file(path: &Path) -> bool {
    let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };

    matches!(
        file_name.to_lowercase().as_str(),
        "cargo.toml"
            | "package.json"
            | "requirements.txt"
            | "build.gradle"
            | "pom.xml"
            | "readme.md"
            | "agents.md"
            | "claude.md"
            | "makefile"
            | "dockerfile"
            | "main.rs"
            | "lib.rs"
            | "index.js"
            | "index.ts"
            | "app.py"
    )
}

/// Generate a high-level summary of the project based on key files.
///
/// Output is byte-stable across calls: `WalkBuilder` doesn't sort siblings
/// (the OS readdir order leaks through), so the joined `key_files` list
/// would otherwise reorder run-to-run on filesystems that don't pre-sort.
/// Only matters when the workspace has no `AGENTS.md` / `CLAUDE.md`, since
/// the system prompt routes through `ProjectContext::as_system_block` first
/// and only falls back here when no project-context document exists.
#[must_use]
pub fn summarize_project(root: &Path) -> String {
    let mut key_files = Vec::new();

    let mut builder = WalkBuilder::new(root);
    builder.hidden(false).follow_links(false).max_depth(Some(2));
    let walker = builder.build();

    for entry in walker {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        if entry.file_type().is_some_and(|ft| ft.is_symlink()) {
            continue;
        }
        if is_key_file(entry.path())
            && let Ok(rel) = entry.path().strip_prefix(root)
        {
            key_files.push(rel.to_string_lossy().to_string());
        }
    }

    key_files.sort();

    if key_files.is_empty() {
        return "Unknown project type".to_string();
    }

    let mut types = Vec::new();
    if key_files
        .iter()
        .any(|f| f.to_lowercase().contains("cargo.toml"))
    {
        types.push("Rust");
    }
    if key_files
        .iter()
        .any(|f| f.to_lowercase().contains("package.json"))
    {
        types.push("JavaScript/Node.js");
    }
    if key_files
        .iter()
        .any(|f| f.to_lowercase().contains("requirements.txt"))
    {
        types.push("Python");
    }

    if types.is_empty() {
        format!("Project with key files: {}", key_files.join(", "))
    } else {
        format!("A {} project", types.join(" and "))
    }
}

/// Generate a tree-like view of the project structure.
///
/// Sibling order is fixed by sorting collected paths — the underlying
/// `WalkBuilder` follows the OS readdir order, which is non-deterministic
/// across filesystems. Sorting by full path preserves the tree shape (a
/// directory still precedes its children because `"src" < "src/lib.rs"`)
/// while making the rendered output byte-stable across runs.
#[must_use]
pub fn project_tree(root: &Path, max_depth: usize, follow_symlinks: bool) -> String {
    let mut entries: Vec<(PathBuf, bool)> = Vec::new();

    let mut builder = WalkBuilder::new(root);
    builder
        .hidden(false)
        .follow_links(follow_symlinks)
        .max_depth(Some(max_depth + 1));

    for entry in builder.build().flatten() {
        if entry.file_type().is_some_and(|ft| ft.is_symlink()) && !follow_symlinks {
            continue;
        }
        let depth = entry.depth();
        if depth == 0 || depth > max_depth {
            continue;
        }
        let rel_path = entry
            .path()
            .strip_prefix(root)
            .unwrap_or(entry.path())
            .to_path_buf();
        let is_dir = entry.file_type().is_some_and(|ft| ft.is_dir());
        entries.push((rel_path, is_dir));
    }

    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let mut tree_lines = Vec::with_capacity(entries.len());
    for (rel_path, is_dir) in entries {
        let depth = rel_path.components().count();
        let indent = "  ".repeat(depth.saturating_sub(1));
        let prefix = if is_dir { "DIR: " } else { "FILE: " };
        tree_lines.push(format!(
            "{}{}{}",
            indent,
            prefix,
            rel_path.file_name().unwrap_or_default().to_string_lossy()
        ));
    }

    tree_lines.join("\n")
}

// === Filesystem Helpers ===

/// Atomically write `contents` to `path` using a temporary file + fsync + rename.
///
/// 1. Creates a `NamedTempFile` in the same directory as `path` (same filesystem).
/// 2. Writes `contents` to the temp file.
/// 3. Calls `sync_all()` on the temp file for durability.
/// 4. Atomically renames (persists) the temp file over `path`.
///
/// On filesystems that support it (`ext4`, `apfs`, `ntfs`), the rename is
/// atomic — a concurrent reader sees either the old content or the new, never
/// a partial write. `sync_all` ensures the data is on stable storage before
/// the metadata change so an OS crash mid-rename doesn't lose data.
///
/// # Errors
/// Returns `io::Error` if the parent directory cannot be determined, the temp
/// file cannot be created, the write fails, or the rename fails.
pub fn write_atomic(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("path has no parent directory: {}", path.display()),
        )
    })?;
    // Use parent directory so the rename is on the same filesystem.
    let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
    std::io::Write::write_all(&mut tmp, contents)?;
    tmp.as_file().sync_all()?;
    tmp.persist(path)?;
    Ok(())
}

/// Open or create a file for appending at `path`, optionally syncing after
/// every write. Use this for append-only logs like `audit.log`.
///
/// The returned `BufWriter<fs::File>` wraps the append handle. Call
/// `.flush()` followed by `.get_ref().sync_all()` after each batch.
pub fn open_append(path: &Path) -> std::io::Result<std::io::BufWriter<std::fs::File>> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    Ok(std::io::BufWriter::new(file))
}

/// Flush a `BufWriter` wrapping a `File`, then `fsync` the underlying file.
pub fn flush_and_sync(writer: &mut std::io::BufWriter<std::fs::File>) -> std::io::Result<()> {
    writer.flush()?;
    writer.get_ref().sync_all()
}

/// Open a URL in the system's default browser.
///
/// Dispatches to the platform-appropriate opener:
/// - macOS: `open`
/// - Linux: `xdg-open`
/// - Windows: `cmd /C start ""`
/// - Other: returns an error.
///
/// This is the single entry point for URL opening — every call site in
/// the codebase should use this instead of hardcoding `Command::new("open")`,
/// `Command::new("xdg-open")`, or `Command::new("cmd")`.
pub fn open_url(url: &str) -> Result<()> {
    let mut command = browser_open_command(url)?;
    command
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| anyhow::anyhow!("failed to launch browser command: {e}"))
}

fn browser_open_command(url: &str) -> Result<Command> {
    if url.trim().is_empty() {
        return Err(anyhow::anyhow!("browser URL cannot be empty"));
    }

    #[cfg(target_os = "macos")]
    {
        let mut command = Command::new("open");
        command.arg(url);
        Ok(command)
    }

    #[cfg(all(target_os = "linux", not(target_env = "ohos")))]
    {
        let mut command = Command::new("xdg-open");
        command.arg(url);
        Ok(command)
    }

    #[cfg(target_os = "windows")]
    {
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", "start", "", url]);
        Ok(cmd)
    }

    #[cfg(not(any(
        target_os = "macos",
        all(target_os = "linux", not(target_env = "ohos")),
        target_os = "windows"
    )))]
    Err(anyhow::anyhow!(
        "browser opening is unsupported on this platform"
    ))
}

/// Spawn a tokio task with panic supervision.
///
/// Wraps the future in `AssertUnwindSafe` + `catch_unwind`. On panic:
/// 1. Logs the panic with the task name and caller location via `tracing::error!`.
/// 2. Writes a crash dump to `~/.mimofanfan/crashes/<timestamp>-<name>.log`.
///
/// The returned `JoinHandle` resolves to `()` — the panic is caught and
/// handled internally so the parent process stays alive.
pub fn spawn_supervised<F>(
    name: &'static str,
    location: &'static std::panic::Location<'static>,
    future: F,
) -> tokio::task::JoinHandle<()>
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    tokio::spawn(async move {
        use futures_util::FutureExt;
        let result = std::panic::AssertUnwindSafe(future).catch_unwind().await;
        if let Err(panic_info) = result {
            let msg = panic_message(&*panic_info);
            tracing::error!(
                target: "panic",
                "Task '{name}' panicked at {}: {msg}",
                location,
            );
            // Write crash dump (best-effort)
            let _ = write_panic_dump(name, location, &msg);
        }
    })
}

/// Extract a human-readable message from a caught panic payload (the `Err`
/// value of `catch_unwind`). Mirrors how the panic hook formats `&str` and
/// `String` payloads so crash dumps stay consistent across call sites.
#[must_use]
pub fn panic_message(panic: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = panic.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = panic.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_string()
    }
}

/// Record a panic that was caught at a call site (via `catch_unwind`) rather
/// than by a task supervisor. Logs it on the `panic` target and writes a
/// best-effort crash dump to `~/.mimofanfan/crashes/`, so diagnostics land in
/// the same place `spawn_supervised` writes them even when the caller recovers
/// and keeps running.
#[track_caller]
pub fn record_caught_panic(name: &'static str, message: &str) {
    let location = std::panic::Location::caller();
    tracing::error!(target: "panic", "Task '{name}' panicked at {location}: {message}");
    let _ = write_panic_dump(name, location, message);
}

/// Write a panic dump file to `~/.mimofanfan/crashes/`.
///
/// Creates the directory if needed and writes a timestamped log
/// with the task name, caller location, and panic message.
/// Best-effort — failures are silently ignored.
fn write_panic_dump(
    name: &str,
    location: &std::panic::Location<'_>,
    message: &str,
) -> std::io::Result<()> {
    let home = dirs::home_dir().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "home directory not found")
    })?;
    let crash_dir = home.join(".mimofan").join("crashes");
    let _ = std::fs::create_dir_all(&crash_dir);
    write_panic_dump_to(&crash_dir, name, location, message)
}

fn write_panic_dump_to(
    crash_dir: &Path,
    name: &str,
    location: &std::panic::Location<'_>,
    message: &str,
) -> std::io::Result<()> {
    use chrono::Utc;
    std::fs::create_dir_all(crash_dir)?;
    let timestamp = Utc::now().format("%Y%m%dT%H%M%S%.3fZ");
    let filename = format!("{timestamp}-{name}.log");
    let path = crash_dir.join(&filename);
    let contents =
        format!("Task: {name}\nLocation: {location}\nTimestamp: {timestamp}\nPanic: {message}\n");
    std::fs::write(&path, contents)?;
    Ok(())
}

/// Fire-and-forget `spawn_blocking` with panic dump protection.
///
/// In contrast to `spawn_supervised` (which wraps `tokio::spawn` for async
/// tasks), this helper wraps `tokio::task::spawn_blocking`.  Use it when a
/// CPU-bound or blocking-I/O task must run off the async runtime and its
/// completion is *not* awaited — for example a post-turn disk snapshot or a
/// file-tree build polled later via a shared data structure.  If the closure
/// panics, a crash dump is written to `~/.mimofanfan/crashes/` and the panic
/// is logged at ERROR level rather than being silently swallowed.
#[track_caller]
pub fn spawn_blocking_supervised<F>(name: &'static str, f: F) -> tokio::task::JoinHandle<()>
where
    F: FnOnce() + Send + 'static,
{
    let location = std::panic::Location::caller();
    tokio::task::spawn_blocking(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
        if let Err(panic_info) = result {
            let msg = panic_message(&*panic_info);
            tracing::error!(
                target: "panic",
                "Blocking task '{name}' panicked at {location}: {msg}",
            );
            let _ = write_panic_dump(name, location, &msg);
        }
    })
}

#[allow(dead_code)]
pub fn ensure_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path)
        .with_context(|| format!("Failed to create directory: {}", path.display()))
}

/// Render JSON with pretty formatting, falling back to a compact string on error.
#[must_use]
#[allow(dead_code)]
pub fn pretty_json(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

/// Truncate a string to a maximum length, adding an ellipsis if truncated.
///
/// Uses char boundaries to avoid panicking on multi-byte UTF-8 characters.
#[must_use]
pub fn truncate_with_ellipsis(s: &str, max_len: usize, ellipsis: &str) -> String {
    if s.len() <= max_len {
        return s.to_string();
    }
    let budget = max_len.saturating_sub(ellipsis.len());
    // Find the last char boundary that fits within the byte budget.
    let safe_end = s
        .char_indices()
        .map(|(i, _)| i)
        .take_while(|&i| i <= budget)
        .last()
        .unwrap_or(0);
    format!("{}{}", &s[..safe_end], ellipsis)
}

/// Percent-encode a string for use in URL query parameters.
///
/// Encodes all characters except unreserved characters (A-Z, a-z, 0-9, `-`, `_`, `.`, `~`).
/// Spaces are encoded as `+`.
#[must_use]
pub fn url_encode(input: &str) -> String {
    let mut encoded = String::new();
    for ch in input.bytes() {
        match ch {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(ch as char)
            }
            b' ' => encoded.push('+'),
            _ => encoded.push_str(&format!("%{ch:02X}")),
        }
    }
    encoded
}

/// Render a path for **user-facing display** with the home directory
/// contracted to `~`. Use this in the TUI, doctor/setup stdout, and any
/// other place a viewer might see the output (screenshot, video,
/// pasted-into-issue help). On macOS/Linux the absolute path
/// `/Users/<name>/...` or `/home/<name>/...` reveals the OS account name,
/// which is often the same as a public handle — undesirable for users
/// who share their terminal.
///
/// **Do not use** this for paths that get persisted (sessions, audit log)
/// or sent to the LLM provider — those want full fidelity so they
/// resolve correctly across processes.
#[must_use]
pub fn display_path(path: &Path) -> String {
    display_path_with_home(path, dirs::home_dir().as_deref())
}

/// Like [`display_path`] but takes an explicit home directory instead of
/// reading `$HOME` / `dirs::home_dir()`.  Used in tests and anywhere the
/// caller already has the home path available.
///
/// The home-relative suffix is rejoined with the platform separator
/// (`\` on Windows, `/` elsewhere) by walking the path's components, so
/// inputs that carried foreign separators don't leak through.
#[must_use]
pub fn display_path_with_home(path: &Path, home: Option<&Path>) -> String {
    let Some(home) = home else {
        return path.display().to_string();
    };
    if let Ok(rest) = path.strip_prefix(home) {
        if rest.as_os_str().is_empty() {
            return "~".to_string();
        }
        let sep = std::path::MAIN_SEPARATOR_STR;
        let mut out = String::from("~");
        for component in rest.components() {
            out.push_str(sep);
            out.push_str(&component.as_os_str().to_string_lossy());
        }
        return out;
    }
    path.display().to_string()
}

/// Estimate the total character count across message content blocks.
#[must_use]
pub fn estimate_message_chars(messages: &[Message]) -> usize {
    let mut total = 0;
    for msg in messages {
        for block in &msg.content {
            match block {
                ContentBlock::Text { text, .. } => total += text.len(),
                ContentBlock::Thinking { thinking, .. } => total += thinking.len(),
                ContentBlock::ToolUse { input, .. } => total += input.to_string().len(),
                ContentBlock::ToolResult { content, .. } => total += content.len(),
                ContentBlock::ServerToolUse { .. }
                | ContentBlock::ToolSearchToolResult { .. }
                | ContentBlock::CodeExecutionToolResult { .. }
                | ContentBlock::ImageUrl { .. } => {}
            }
        }
    }
    total
}

// Tests use `display_path_with_home` so they never mutate the global `HOME`
// env var.  Mutating `HOME` via `std::env::set_var` is not thread-safe; Cargo
// runs tests in parallel by default and CI runners are multi-core, so any test
// that stomps `HOME` will race with tests that *read* it.  Using the injected
// helper avoids the race entirely and makes the tests portable to Windows
// without additional platform scaffolding.
#[cfg(test)]
mod tests {}

#[cfg(test)]
mod atomic_write_tests {}

#[cfg(test)]
mod spawn_supervised_tests {}

#[cfg(test)]
mod project_mapping_tests {}
