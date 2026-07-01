//! External-binary dependency resolution for tools that shell out to
//! locally-installed programs (Python for `code_execution` / RLM REPL,
//! `pdftotext` for PDF reading in `read_file`, future tools as added).
//!
//! Before v0.8.31, tools that called external binaries hardcoded the
//! command name and failed at execution time when the binary wasn't on
//! `PATH`. The most-cited example was `code_execution`, which spawned
//! `python3` directly — Windows users (where the launcher is `py` or
//! `python`, not `python3`) saw `Failed to execute tool: program not
//! found` with no upstream hint of what was wrong.
//!
//! This module centralises the probe-then-decide pattern. The supported
//! callers today are:
//!
//! - Tool catalog construction (`core::engine::tool_catalog`): for
//!   tools that should be advertised to the model only when the
//!   required runtime is present.
//! - Doctor command (`run_doctor` in `main.rs`): for surfacing the
//!   resolved state to the user so missing dependencies aren't an
//!   invisible failure.
//! - Long-lived REPL runtime (`repl::runtime`): for RLM and inline `repl`
//!   blocks that need to spawn Python on every supported platform.
//!
//! Results are cached for the process lifetime via [`std::sync::OnceLock`]
//! — probing a binary involves a `Command::output` per candidate and
//! we'd rather not pay that on every model turn.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// Candidate executable names for the Python interpreter, in the
/// order we try them. On Windows the launcher convention is `py -3`,
/// so we add it as a third option; the resolver splits on whitespace
/// at execution time so `py -3 /tmp/code.py` runs correctly.
///
/// Order matters: `python3` first because it's the unambiguous v3
/// binary on Unix and rules out Python 2 leftovers. `python` second
/// covers Windows installations that drop the version suffix and
/// modern macOS where Homebrew installs both. `py -3` last as a
/// Windows-launcher fallback.
pub const PYTHON_CANDIDATES: &[&str] = &["python3", "python", "py -3"];

/// Probe a single executable. Returns `true` when the candidate
/// responds to `--version` with a successful exit. Splits on
/// whitespace so `"py -3"` works as a candidate.
///
/// We deliberately use `--version` rather than `which` so the probe
/// is portable across Unix, Windows (no `which` by default), and
/// containers. The downside is that we spawn a subprocess per
/// candidate; the resolver caches the result so this only fires
/// once per process.
#[must_use]
pub fn probe_executable(spec: &str) -> bool {
    probe_executable_with_flag(spec, "--version")
}

/// Probe a single executable using an explicit version/help flag.
///
/// Most tools report their presence via `--version`, but some do not:
/// Poppler's `pdftotext` treats `--version` as an input *filename* and
/// exits non-zero ("I/O Error: Couldn't open file '--version'"), so the
/// default probe reports it missing even when it is installed (#1667).
/// Such tools pass their own flag (e.g. `-v`) here.
#[must_use]
pub fn probe_executable_with_flag(spec: &str, version_flag: &str) -> bool {
    let mut parts = spec.split_whitespace();
    let Some(program) = parts.next() else {
        return false;
    };
    let mut cmd = Command::new(program);
    for arg in parts {
        cmd.arg(arg);
    }
    cmd.arg(version_flag);

    // Silence the subprocess's stdout/stderr — the version banner would
    // otherwise print to our terminal during startup, which is
    // confusing on the TUI's first frame.
    cmd.stdout(std::process::Stdio::null());
    cmd.stderr(std::process::Stdio::null());

    matches!(cmd.status(), Ok(status) if status.success())
}

fn executable_path_candidates(program: &str) -> Vec<PathBuf> {
    let program_path = Path::new(program);
    if program_path.components().count() > 1 {
        return vec![program_path.to_path_buf()];
    }

    let Some(path) = std::env::var_os("PATH") else {
        return vec![PathBuf::from(program)];
    };

    let mut candidates = Vec::new();
    for dir in std::env::split_paths(&path) {
        let bare = dir.join(program);
        candidates.push(bare.clone());

        #[cfg(windows)]
        if Path::new(program).extension().is_none() {
            let pathext =
                std::env::var_os("PATHEXT").unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".into());
            for ext in pathext.to_string_lossy().split(';') {
                if ext.is_empty() {
                    continue;
                }
                candidates.push(bare.with_extension(ext.trim_start_matches('.')));
            }
        }
    }

    candidates
}

fn resolve_executable_path(spec: &str, version_flag: &str) -> Option<String> {
    let mut parts = spec.split_whitespace();
    let program = parts.next()?;
    let args: Vec<&str> = parts.collect();

    for candidate in executable_path_candidates(program) {
        if !candidate.is_file() {
            continue;
        }

        let mut cmd = Command::new(&candidate);
        cmd.args(&args)
            .arg(version_flag)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());

        if matches!(cmd.status(), Ok(status) if status.success()) {
            return Some(candidate.to_string_lossy().into_owned());
        }
    }

    None
}

/// Resolve the Python interpreter once per process. Returns the
/// candidate spec (e.g. `"python3"` or `"py -3"`) that succeeded,
/// or `None` when every candidate failed.
///
/// Callers that need to spawn the interpreter should split this
/// string on whitespace — see [`split_interpreter_spec`].
pub fn resolve_python_interpreter() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            for candidate in PYTHON_CANDIDATES {
                if probe_executable(candidate) {
                    tracing::info!(
                        target: "tool_dependencies",
                        candidate = candidate,
                        "Resolved Python interpreter",
                    );
                    return Some((*candidate).to_string());
                }
            }
            tracing::warn!(
                target: "tool_dependencies",
                tried = ?PYTHON_CANDIDATES,
                "No Python interpreter found",
            );
            None
        })
        .clone()
}

/// Resolve `pdftotext` (from Poppler) once per process. Used by
/// `read_file`'s PDF path for graceful fallback messaging. Unlike
/// the Python case, `read_file` itself still works for text files
/// when `pdftotext` is missing — this resolver exists so the doctor
/// command can surface the miss explicitly rather than the user
/// hitting "PDF unsupported" on a read attempt.
pub fn resolve_pdftotext() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            // Poppler's `pdftotext` rejects `--version` (it is parsed as an
            // input filename and exits non-zero), so probe with `-v`, which
            // prints the version banner and exits 0 (#1667).
            if probe_executable_with_flag("pdftotext", "-v") {
                Some("pdftotext".to_string())
            } else {
                None
            }
        })
        .clone()
}

/// Resolve `tesseract` (OCR engine) once per process. Used by the
/// `image_ocr` tool on platforms that do not have a native OCR backend.
/// Tesseract is the de-facto open-source OCR engine and ships as a single
/// binary on every platform we support, so the candidate list is just
/// `tesseract`.
pub fn resolve_tesseract() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            if probe_executable("tesseract") {
                tracing::info!(
                    target: "tool_dependencies",
                    "Resolved tesseract binary for image_ocr",
                );
                Some("tesseract".to_string())
            } else {
                tracing::warn!(
                    target: "tool_dependencies",
                    "tesseract binary not found; image_ocr will rely on native OCR if available",
                );
                None
            }
        })
        .clone()
}

/// Resolve `pandoc` (universal document converter) once per
/// process. Used by the `pandoc_convert` tool to decide whether
/// to register itself with the model. Pandoc is a single-binary
/// install, so the candidate list is just `pandoc` — no platform
/// fallback path.
pub fn resolve_pandoc() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            if let Some(path) = resolve_executable_path("pandoc", "--version") {
                tracing::info!(
                    target: "tool_dependencies",
                    "Resolved pandoc binary for pandoc_convert",
                );
                Some(path)
            } else {
                tracing::warn!(
                    target: "tool_dependencies",
                    "pandoc binary not found; pandoc_convert tool will not be registered",
                );
                None
            }
        })
        .clone()
}

/// Resolve the Node.js runtime once per process. Used by the
/// `js_execution` tool to decide whether to advertise itself in
/// the catalog. Unlike Python, the executable name `node` is the
/// same across every platform we ship to — there's no `node3` or
/// `node.exe` variant to fall through to — so this is a single
/// probe rather than a candidate ladder.
pub fn resolve_node() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            if probe_executable("node") {
                tracing::info!(
                    target: "tool_dependencies",
                    "Resolved Node.js runtime for js_execution",
                );
                Some("node".to_string())
            } else {
                tracing::warn!(
                    target: "tool_dependencies",
                    "Node.js runtime not found; js_execution tool will not be advertised",
                );
                None
            }
        })
        .clone()
}

// ---------------------------------------------------------------------------
// ExternalTool trait — unified subprocess interface
// ---------------------------------------------------------------------------

/// A tool that Mimofan shells out to. Instead of scattering
/// `Command::new("git")` / `Command::new("gh")` across the codebase,
/// each external dependency implements this trait once in this module.
/// Callers ask the tool for a pre-populated [`Command`] and chain their
/// own args, working directory, and spawn method.
///
/// # Example
///
/// ```ignore
/// let output = Git::command()
///     .expect("git not found")
///     .args(["diff", "--stat"])
///     .current_dir(&workspace)
///     .output()?;
/// ```
pub trait ExternalTool {
    /// Candidate binary names, tried in order until one responds to
    /// `--version`.  For single-binary tools (git, gh, node) this is a
    /// one-element slice.
    fn candidates() -> &'static [&'static str];

    /// Resolve the best candidate once per process (cached). Returns
    /// the spec string (e.g. `"python3"` or `"py -3"`).
    fn resolve() -> Option<String>;

    /// Quick availability check — true when the tool was found on PATH.
    #[allow(dead_code)]
    fn available() -> bool {
        Self::resolve().is_some()
    }

    /// Build a `std::process::Command` pre-populated with the resolved
    /// binary (and any fixed arguments from a multi-word candidate like
    /// `"py -3"`). Returns `None` when the tool isn't installed.
    ///
    /// Callers should chain `.args(...)`, `.current_dir(...)`, and then
    /// call `.output()`, `.status()`, or `.spawn()`.
    fn command() -> Option<Command> {
        let spec = Self::resolve()?;
        let (program, fixed_args) = split_interpreter_spec(&spec);
        let mut cmd = Command::new(&program);
        for arg in &fixed_args {
            cmd.arg(arg);
        }
        Some(cmd)
    }

    /// Convenience: run the tool with arguments in a working directory
    /// and return the captured output.
    fn output(args: &[&str], cwd: &std::path::Path) -> std::io::Result<std::process::Output> {
        let mut cmd = Self::command().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("{} not found on PATH", std::any::type_name::<Self>()),
            )
        })?;
        cmd.args(args).current_dir(cwd).output()
    }

    /// Convenience: run the tool with arguments and return only the
    /// exit status (discards stdout/stderr).
    #[allow(dead_code)]
    fn status(args: &[&str], cwd: &std::path::Path) -> std::io::Result<std::process::ExitStatus> {
        let mut cmd = Self::command().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("{} not found on PATH", std::any::type_name::<Self>()),
            )
        })?;
        cmd.args(args).current_dir(cwd).status()
    }

    /// Build a `tokio::process::Command` pre-populated with the resolved
    /// binary (and any fixed arguments from a multi-word candidate like
    /// `"py -3"`). Returns `None` when the tool isn't installed.
    ///
    /// Async callers (`code_execution`, `js_execution`) use this instead
    /// of [`ExternalTool::command`] so they can `.await` the child.
    fn tokio_command() -> Option<tokio::process::Command> {
        let spec = Self::resolve()?;
        let (program, fixed_args) = split_interpreter_spec(&spec);
        let mut cmd = tokio::process::Command::new(&program);
        for arg in &fixed_args {
            cmd.arg(arg);
        }
        Some(cmd)
    }
}

// ---------------------------------------------------------------------------
// Concrete tool implementations
// ---------------------------------------------------------------------------

/// Git version control.
pub struct Git;

impl ExternalTool for Git {
    fn candidates() -> &'static [&'static str] {
        &["git"]
    }

    fn resolve() -> Option<String> {
        static CACHE: OnceLock<Option<String>> = OnceLock::new();
        CACHE
            .get_or_init(|| {
                for candidate in Self::candidates() {
                    if probe_executable(candidate) {
                        tracing::info!(target: "tool_dependencies", "Resolved git binary");
                        return Some((*candidate).to_string());
                    }
                }
                None
            })
            .clone()
    }
}

/// GitHub CLI.
pub struct Gh;

impl ExternalTool for Gh {
    fn candidates() -> &'static [&'static str] {
        &["gh"]
    }

    fn resolve() -> Option<String> {
        static CACHE: OnceLock<Option<String>> = OnceLock::new();
        CACHE
            .get_or_init(|| {
                for candidate in Self::candidates() {
                    if probe_executable(candidate) {
                        tracing::info!(target: "tool_dependencies", "Resolved gh binary");
                        return Some((*candidate).to_string());
                    }
                }
                None
            })
            .clone()
    }
}

/// Rust compiler — used for version reporting in diagnostics.
pub struct RustC;

impl ExternalTool for RustC {
    fn candidates() -> &'static [&'static str] {
        &["rustc"]
    }

    fn resolve() -> Option<String> {
        static CACHE: OnceLock<Option<String>> = OnceLock::new();
        CACHE
            .get_or_init(|| {
                for candidate in Self::candidates() {
                    if probe_executable(candidate) {
                        tracing::info!(target: "tool_dependencies", "Resolved rustc binary");
                        return Some((*candidate).to_string());
                    }
                }
                None
            })
            .clone()
    }
}

/// Rust build tool — used by the `run_tests` tool.
pub struct Cargo;

impl ExternalTool for Cargo {
    fn candidates() -> &'static [&'static str] {
        &["cargo"]
    }

    fn resolve() -> Option<String> {
        static CACHE: OnceLock<Option<String>> = OnceLock::new();
        CACHE
            .get_or_init(|| {
                for candidate in Self::candidates() {
                    if probe_executable(candidate) {
                        tracing::info!(target: "tool_dependencies", "Resolved cargo binary");
                        return Some((*candidate).to_string());
                    }
                }
                None
            })
            .clone()
    }
}

/// Python interpreter — used by `code_execution` tool and RLM REPL.
/// Delegates to the existing [`resolve_python_interpreter`] so the
/// multi-candidate ladder (`python3` → `python` → `py -3`) is
/// shared with legacy callers until they migrate to the trait.
pub struct Python;

impl ExternalTool for Python {
    fn candidates() -> &'static [&'static str] {
        PYTHON_CANDIDATES
    }

    fn resolve() -> Option<String> {
        resolve_python_interpreter()
    }
}

/// Node.js runtime — used by the `js_execution` tool.
/// The binary name `node` is the same on every platform we support,
/// so this is a single probe rather than a candidate ladder.
pub struct Node;

impl ExternalTool for Node {
    fn candidates() -> &'static [&'static str] {
        &["node"]
    }

    fn resolve() -> Option<String> {
        resolve_node()
    }
}

// ---------------------------------------------------------------------------
// Legacy interpreter helpers (kept for existing callers until migrated)
// ---------------------------------------------------------------------------

/// Split an interpreter spec like `"py -3"` into the program name
/// and any initial arguments. Returns `("py", vec!["-3"])` for the
/// example; returns `("python3", vec![])` for a bare name.
///
/// Callers spawn `Command::new(program).args(args).arg(script_path)`.
#[must_use]
pub fn split_interpreter_spec(spec: &str) -> (String, Vec<String>) {
    let mut parts = spec.split_whitespace();
    let program = parts.next().unwrap_or("").to_string();
    let args = parts.map(str::to_string).collect();
    (program, args)
}

#[cfg(test)]
mod tests {}
