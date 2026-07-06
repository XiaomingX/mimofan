//! Sanitized environment handling for child processes.

use std::collections::HashMap;
use std::ffi::{OsStr, OsString};

/// Convert a string env map into owned OS strings for child env helpers.
pub fn string_map_env(
    env: &HashMap<String, String>,
) -> impl Iterator<Item = (OsString, OsString)> + '_ {
    env.iter()
        .map(|(key, value)| (OsString::from(key), OsString::from(value)))
}

/// Return the environment for a child process after dropping parent secrets.
///
/// `overrides` are trusted call-site values, such as sandbox markers, hook
/// variables, MCP server config, or RLM context path. They are applied after the
/// parent allowlist so explicit values win.
pub fn sanitized_child_env<I, K, V>(overrides: I) -> Vec<(OsString, OsString)>
where
    I: IntoIterator<Item = (K, V)>,
    K: AsRef<OsStr>,
    V: AsRef<OsStr>,
{
    let mut env = Vec::new();
    for (key, value) in std::env::vars_os() {
        if is_allowed_parent_env_key(&key) {
            upsert_env(&mut env, key, value);
        }
    }
    for (key, value) in overrides {
        upsert_env(
            &mut env,
            key.as_ref().to_os_string(),
            value.as_ref().to_os_string(),
        );
    }
    #[cfg(windows)]
    fill_windows_common_program_files(&mut env);
    env
}

pub fn apply_to_command<I, K, V>(cmd: &mut std::process::Command, overrides: I)
where
    I: IntoIterator<Item = (K, V)>,
    K: AsRef<OsStr>,
    V: AsRef<OsStr>,
{
    cmd.env_clear();
    for (key, value) in sanitized_child_env(overrides) {
        cmd.env(key, value);
    }
}

pub fn apply_to_tokio_command<I, K, V>(cmd: &mut tokio::process::Command, overrides: I)
where
    I: IntoIterator<Item = (K, V)>,
    K: AsRef<OsStr>,
    V: AsRef<OsStr>,
{
    cmd.env_clear();
    for (key, value) in sanitized_child_env(overrides) {
        cmd.env(key, value);
    }
}

#[cfg(not(target_env = "ohos"))]
pub fn apply_to_pty_command<I, K, V>(cmd: &mut portable_pty::CommandBuilder, overrides: I)
where
    I: IntoIterator<Item = (K, V)>,
    K: AsRef<OsStr>,
    V: AsRef<OsStr>,
{
    cmd.env_clear();
    for (key, value) in sanitized_child_env(overrides) {
        cmd.env(key, value);
    }
}

/// Build the sanitized child environment used for MCP stdio servers.
///
/// MCP stdio servers are user-configured integrations declared in
/// `~/.mimofanfan/mcp.json` (or equivalent). They are not arbitrary processes
/// the agent decided to launch on its own. To avoid breaking common
/// `npx ...` / `uvx ...` / `python -m mcp_server_*` setups (#1244), the
/// MCP-launch allowlist is wider than the base shell-tool allowlist: it
/// also passes through Node, npm, Python, Ruby, Java, proxy, and CA-bundle
/// bootstrap variables. It still drops arbitrary parent env so secret-bearing
/// vars (`AWS_*`, `*_API_KEY`, `GITHUB_TOKEN`, …) are not silently exported.
pub fn sanitized_mcp_env<I, K, V>(overrides: I) -> Vec<(OsString, OsString)>
where
    I: IntoIterator<Item = (K, V)>,
    K: AsRef<OsStr>,
    V: AsRef<OsStr>,
{
    let mut env = Vec::new();
    for (key, value) in std::env::vars_os() {
        if is_allowed_mcp_env_key(&key) {
            upsert_env(&mut env, key, value);
        }
    }
    for (key, value) in overrides {
        upsert_env(
            &mut env,
            key.as_ref().to_os_string(),
            value.as_ref().to_os_string(),
        );
    }
    env
}

pub fn apply_to_tokio_command_mcp<I, K, V>(cmd: &mut tokio::process::Command, overrides: I)
where
    I: IntoIterator<Item = (K, V)>,
    K: AsRef<OsStr>,
    V: AsRef<OsStr>,
{
    cmd.env_clear();
    for (key, value) in sanitized_mcp_env(overrides) {
        cmd.env(key, value);
    }
}

fn is_allowed_parent_env_key(key: &OsStr) -> bool {
    let key = key.to_string_lossy();
    let normalized = key.to_ascii_uppercase();
    matches!(
        normalized.as_str(),
        "PATH"
            | "HOME"
            | "USER"
            | "USERNAME"
            | "LOGNAME"
            | "LANG"
            | "LANGUAGE"
            | "LC_ALL"
            | "LC_CTYPE"
            | "LC_MESSAGES"
            | "TERM"
            | "COLORTERM"
            | "NO_COLOR"
            | "FORCE_COLOR"
            | "SHELL"
            | "TMPDIR"
            | "TMP"
            | "TEMP"
            | "__CF_USER_TEXT_ENCODING"
            | "SYSTEMROOT"
            | "WINDIR"
            | "COMSPEC"
            | "PATHEXT"
            | "USERPROFILE"
            | "HOMEDRIVE"
            | "HOMEPATH"
            // Preserve Windows toolchain context when the parent shell has
            // already loaded VsDevCmd / vcvars. Without these, `exec_shell`
            // can find `link.exe` via PATH but still fail to resolve
            // SDK/CRT libraries like `kernel32.lib`, so any model-driven
            // `cargo build` from inside the TUI silently breaks on
            // Windows installs that don't run inside a Developer Command
            // Prompt. Harvested from PR #1487.
            | "LIB"
            | "LIBPATH"
            | "INCLUDE"
            | "VSINSTALLDIR"
            | "VCINSTALLDIR"
            | "VCTOOLSINSTALLDIR"
            | "WINDOWSSDKDIR"
            | "WINDOWSSDKVERSION"
            | "UNIVERSALCRTSDKDIR"
            | "UCRTVERSION"
            | "EXTENSIONSDKDIR"
            | "DEVENVDIR"
            | "VISUALSTUDIOVERSION"
            // Windows app-data + .NET/NuGet paths. `dotnet restore` (and npm,
            // pip, etc.) resolve their package caches, HTTP cache, and config
            // under %APPDATA% / %LOCALAPPDATA% / %ProgramData% / %ProgramFiles%.
            // The sanitized child env dropped these, so restore failed through
            // `exec_shell` even though it worked in the user's own shell, where
            // the full environment is present (#1857). `DOTNET_*` (below) covers
            // DOTNET_ROOT and the CLI flags.
            | "APPDATA"
            | "LOCALAPPDATA"
            | "PROGRAMDATA"
            | "ALLUSERSPROFILE"
            | "PROGRAMFILES"
            | "PROGRAMFILES(X86)"
            | "PROGRAMW6432"
            | "COMMONPROGRAMFILES"
            | "COMMONPROGRAMFILES(X86)"
            | "COMMONPROGRAMW6432"
            | "PROCESSOR_ARCHITECTURE"
            | "NUGET_PACKAGES"
            | "NUGET_HTTP_CACHE_PATH"
            // Standard proxy variables are needed by shell tasks in
            // corporate and WSL environments where direct internet egress is
            // blocked. They intentionally exclude token/API-key-shaped vars.
            | "HTTP_PROXY"
            | "HTTPS_PROXY"
            | "NO_PROXY"
            | "ALL_PROXY"
            | "FTP_PROXY"
    ) || normalized.starts_with("LC_")
        // .NET CLI / SDK configuration (DOTNET_ROOT, DOTNET_CLI_*,
        // DOTNET_NOLOGO, DOTNET_CLI_TELEMETRY_OPTOUT, …). Paths and flags
        // only — no secret-shaped values (#1857).
        || normalized.starts_with("DOTNET_")
}

/// Allowlist for MCP stdio launches. Strict superset of
/// `is_allowed_parent_env_key`. See `sanitized_mcp_env` for rationale.
fn is_allowed_mcp_env_key(key: &OsStr) -> bool {
    if is_allowed_parent_env_key(key) {
        return true;
    }
    let key_str = key.to_string_lossy();
    let normalized = key_str.to_ascii_uppercase();
    if matches!(
        normalized.as_str(),
        // Node.js / npm / npx / pnpm / yarn / volta / corepack
        "NVM_DIR"
            | "NVM_BIN"
            | "NVM_INC"
            | "VOLTA_HOME"
            | "COREPACK_HOME"
            | "NODE_PATH"
            | "NODE_OPTIONS"
            | "NODE_EXTRA_CA_CERTS"
            // Python ecosystem
            | "PYTHONPATH"
            | "PYTHONHOME"
            | "PYTHONDONTWRITEBYTECODE"
            | "PYTHONUNBUFFERED"
            | "VIRTUAL_ENV"
            | "POETRY_HOME"
            | "PIPX_HOME"
            | "PIPX_BIN_DIR"
            // Ruby ecosystem
            | "GEM_HOME"
            | "GEM_PATH"
            | "BUNDLE_PATH"
            | "BUNDLE_GEMFILE"
            // Java
            | "JAVA_HOME"
            // Network proxies (uppercase form; lowercase handled below)
            | "HTTP_PROXY"
            | "HTTPS_PROXY"
            | "NO_PROXY"
            | "ALL_PROXY"
            | "FTP_PROXY"
            // Custom CA bundles for corporate TLS interception
            | "SSL_CERT_FILE"
            | "SSL_CERT_DIR"
            | "REQUESTS_CA_BUNDLE"
            | "CURL_CA_BUNDLE"
    ) {
        return true;
    }
    // npm config namespace (NPM_CONFIG_PREFIX, NPM_CONFIG_CACHE, …) and
    // uv (UV_CACHE_DIR, UV_PYTHON, …) — both ecosystems use a stable prefix
    // for their bootstrap configuration, so allow the whole namespace.
    if normalized.starts_with("NPM_CONFIG_") || normalized.starts_with("UV_") {
        return true;
    }
    false
}

fn upsert_env(env: &mut Vec<(OsString, OsString)>, key: OsString, value: OsString) {
    let normalized = normalize_key(&key);
    env.retain(|(existing, _)| normalize_key(existing) != normalized);
    env.push((key, value));
}

#[cfg(any(windows, test))]
fn fill_windows_common_program_files(env: &mut Vec<(OsString, OsString)>) {
    for (key, default) in [
        ("CommonProgramFiles", r"C:\Program Files\Common Files"),
        (
            "CommonProgramFiles(x86)",
            r"C:\Program Files (x86)\Common Files",
        ),
        ("CommonProgramW6432", r"C:\Program Files\Common Files"),
    ] {
        let existing = env
            .iter()
            .find(|(existing, _)| normalize_key(existing) == normalize_key(OsStr::new(key)))
            .map(|(_, value)| value.to_string_lossy().trim().is_empty());
        if existing.unwrap_or(true) {
            upsert_env(env, OsString::from(key), OsString::from(default));
        }
    }
}

fn normalize_key(key: &OsStr) -> String {
    key.to_string_lossy().to_ascii_uppercase()
}

#[cfg(test)]
mod tests {}
