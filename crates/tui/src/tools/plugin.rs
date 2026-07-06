//! Plugin tool system — scripts and commands as first-class tools.
//!
//! Users can drop self-describing scripts in `~/.mimofanfan/tools/` and they
//! are auto-discovered, parsed for frontmatter, and registered as model-visible
//! tools alongside built-in implementations.
//!
//! # Script frontmatter format
//!
//! Every plugin script must have a frontmatter header in its first 20 lines:
//!
//! ```sh
//! # name: my-tool
//! # description: Does something useful
//! # schema: {"type":"object","properties":{"input":{"type":"string"}}}
//! # approval: auto
//! ```
//!
//! The script receives the tool's JSON input on **stdin** and must return
//! a JSON `ToolResult` (`{"content": "...", "success": true}`) on **stdout**.
//! Non-JSON output is wrapped in a `ToolResult` with `success: false`.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::Value;
use tokio::io::AsyncWriteExt;

use super::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec,
};

use crate::config::ToolOverride;

/// Timeout for plugin script execution (120 seconds).
const PLUGIN_EXECUTION_TIMEOUT: Duration = Duration::from_secs(120);

/// Metadata extracted from a plugin script's frontmatter header.
#[derive(Debug, Clone)]
pub struct PluginMetadata {
    /// Tool name (from `# name:`).
    pub name: String,
    /// Human-readable description (from `# description:`).
    pub description: String,
    /// JSON Schema for the tool's input (from `# schema:`).
    /// Defaults to a permissive `{"type": "object"}` when absent.
    pub input_schema: Value,
    /// Approval requirement (from `# approval:`).
    /// Defaults to `Suggest`.
    pub approval: ApprovalRequirement,
}

/// A tool backed by an external script or executable dropped into the
/// plugins directory. The script receives JSON input on stdin and writes
/// a JSON `ToolResult` to stdout.
struct ScriptPluginTool {
    metadata: PluginMetadata,
    /// Absolute path to the script.
    script_path: PathBuf,
    /// Optional static arguments passed before the JSON input.
    args: Vec<String>,
}

impl std::fmt::Debug for ScriptPluginTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScriptPluginTool")
            .field("name", &self.metadata.name)
            .field("script_path", &self.script_path)
            .finish()
    }
}

#[async_trait]
impl ToolSpec for ScriptPluginTool {
    fn name(&self) -> &str {
        &self.metadata.name
    }

    fn description(&self) -> &str {
        &self.metadata.description
    }

    fn input_schema(&self) -> Value {
        self.metadata.input_schema.clone()
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        // Unknown plugin — conservative: mark as requiring execution + approval.
        vec![
            ToolCapability::ExecutesCode,
            ToolCapability::RequiresApproval,
        ]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        self.metadata.approval
    }

    async fn execute(&self, input: Value, _context: &ToolContext) -> Result<ToolResult, ToolError> {
        let (interpreter, script_args) = script_command_parts(&self.script_path, &self.args);
        let label = self.script_path.display().to_string();
        run_plugin_child(&interpreter, &script_args, &label, input).await
    }
}

/// A tool backed by an arbitrary shell command from config.toml overrides.
/// Behaves like `ScriptPluginTool` but uses the user-specified command string.
struct CommandPluginTool {
    name: String,
    description: String,
    input_schema: Value,
    command: String,
    args: Vec<String>,
    approval: ApprovalRequirement,
}

impl std::fmt::Debug for CommandPluginTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommandPluginTool")
            .field("name", &self.name)
            .field("command", &self.command)
            .finish()
    }
}

#[async_trait]
impl ToolSpec for CommandPluginTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> Value {
        self.input_schema.clone()
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![
            ToolCapability::ExecutesCode,
            ToolCapability::RequiresApproval,
        ]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        self.approval
    }

    async fn execute(&self, input: Value, _context: &ToolContext) -> Result<ToolResult, ToolError> {
        // On Windows, if the command doesn't have an extension, try wrapping
        // in `cmd /c` or use `powershell` for `.ps1` files. For portability
        // we let tokio::process::Command resolve via PATH.
        let mut cmd = if cfg!(windows) && !self.command.contains('.') {
            let mut c = tokio::process::Command::new("cmd");
            c.arg("/c").arg(&self.command);
            c
        } else {
            tokio::process::Command::new(&self.command)
        };
        cmd.args(&self.args);
        let label = format!("command '{}'", self.command);
        run_plugin_child_raw(&mut cmd, &label, input).await
    }
}

// ---------------------------------------------------------------------------
// Script interpreter resolution
// ---------------------------------------------------------------------------

/// Parse a shebang line (`#!/usr/bin/env node`) to extract the interpreter.
fn parse_shebang(path: &Path) -> Option<(String, Vec<String>)> {
    let mut file = std::fs::File::open(path).ok()?;
    let content = read_prefix_to_string(&mut file, 256)?;
    let first_line = content.lines().next()?;
    let rest = first_line.strip_prefix("#!")?;
    let parts: Vec<&str> = rest.split_whitespace().collect();
    if parts.is_empty() {
        return None;
    }
    let interpreter = parts[0].to_string();
    let args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();
    Some((interpreter, args))
}

/// Resolve the interpreter binary and pre-args for a script file.
///
/// Priority:
/// 1. Shebang line from the script itself (`#!/usr/bin/env node`)
/// 2. Extension-based fallback for known script types
/// 3. Direct execution (assumes the OS knows how to run it)
fn resolve_interpreter(path: &Path) -> (String, Vec<String>) {
    // 1. Try shebang
    if let Some((interp, shebang_args)) = parse_shebang(path) {
        let bin_name = interp.rsplit('/').next().unwrap_or(&interp);
        // `env` is a special case: `#!/usr/bin/env node` → `node`
        // On Windows, `env` is not available, so extract the intended binary.
        if bin_name == "env" && !shebang_args.is_empty() {
            return (shebang_args[0].clone(), shebang_args[1..].to_vec());
        }
        if cfg!(windows) {
            return (bin_name.to_string(), shebang_args);
        }
        return (interp, shebang_args);
    }

    // 2. Extension-based fallback for common script types
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    match ext.as_str() {
        "ps1" => ("powershell".into(), vec!["-File".into()]),
        "py" => ("python".into(), vec![]),
        "js" | "mjs" => ("node".into(), vec![]),
        "ts" => ("npx".into(), vec!["tsx".into()]),
        "rb" => ("ruby".into(), vec![]),
        "sh" | "bash" | "zsh" => {
            // On Windows, route shell scripts through sh if available
            if cfg!(windows) {
                ("sh".into(), vec![])
            } else {
                (path.to_string_lossy().into(), vec![])
            }
        }
        _ => (path.to_string_lossy().into(), vec![]),
    }
}

fn script_command_parts(script_path: &Path, args: &[String]) -> (String, Vec<String>) {
    let (interpreter, mut script_args) = resolve_interpreter(script_path);
    let script_path_arg = script_path.to_string_lossy().to_string();
    if interpreter != script_path_arg {
        script_args.push(script_path_arg);
    }
    script_args.extend(args.iter().cloned());
    (interpreter, script_args)
}

fn read_prefix_to_string(reader: impl std::io::Read, max_bytes: u64) -> Option<String> {
    use std::io::Read;

    let mut buf = Vec::new();
    reader.take(max_bytes).read_to_end(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

// ---------------------------------------------------------------------------
// Shared child process helpers
// ---------------------------------------------------------------------------

/// Spawn a command, pipe JSON input to stdin, collect ToolResult from stdout.
async fn run_plugin_child(
    command: &str,
    args: &[String],
    label: &str,
    input: Value,
) -> Result<ToolResult, ToolError> {
    let mut cmd = tokio::process::Command::new(command);
    cmd.args(args);
    run_plugin_child_raw(&mut cmd, label, input).await
}

/// Run a pre-configured tokio Command, pipe JSON input, collect ToolResult.
async fn run_plugin_child_raw(
    cmd: &mut tokio::process::Command,
    label: &str,
    input: Value,
) -> Result<ToolResult, ToolError> {
    let input_bytes = serde_json::to_vec(&input)
        .map_err(|e| ToolError::invalid_input(format!("failed to serialize input: {e}")))?;

    cmd.stdin(std::process::Stdio::piped());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| ToolError::execution_failed(format!("failed to spawn {label}: {e}")))?;

    let stdin_writer = child.stdin.take().map(|mut stdin| {
        tokio::spawn(async move {
            if stdin.write_all(&input_bytes).await.is_ok() {
                let _ = stdin.shutdown().await;
            }
        })
    });

    let output = tokio::time::timeout(PLUGIN_EXECUTION_TIMEOUT, child.wait_with_output())
        .await
        .map_err(|_| ToolError::Timeout {
            seconds: PLUGIN_EXECUTION_TIMEOUT.as_secs(),
        })?
        .map_err(|e| ToolError::execution_failed(format!("process error: {e}")))?;

    if let Some(stdin_writer) = stdin_writer {
        let _ = stdin_writer.await;
    }

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        if let Ok(parsed) = serde_json::from_str::<ToolResult>(&stdout) {
            Ok(parsed)
        } else {
            Ok(ToolResult::success(stdout))
        }
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let combined = if stderr.is_empty() {
            stdout
        } else if stdout.is_empty() {
            stderr
        } else {
            format!("{stdout}\n{stderr}")
        };
        Err(ToolError::execution_failed(combined))
    }
}

// ---------------------------------------------------------------------------
// Frontmatter parsing
// ---------------------------------------------------------------------------

/// Parse frontmatter header from the first `max_lines` lines of a text file.
///
/// Expected format (one `# key: value` per line):
/// ```text
/// # name: my-tool
/// # description: Does something
/// # schema: {"type":"object"}
/// # approval: auto
/// ```
///
/// Also supports `// ` prefix for JavaScript/TypeScript scripts and `-- ` for Lua.
pub fn parse_frontmatter(content: &str) -> PluginMetadata {
    let mut name = String::new();
    let mut description = String::new();
    let mut schema_str = String::new();
    let mut approval_str = String::new();

    for line in content.lines().take(20) {
        let line = line.trim();
        // Strip leading comment markers: `#`, `//`, `--`.
        let rest = line
            .strip_prefix('#')
            .or_else(|| line.strip_prefix("//"))
            .or_else(|| line.strip_prefix("--"));
        let Some(rest) = rest else { continue };
        if let Some((key, value)) = rest.trim_start().split_once(':') {
            let key = key.trim().to_lowercase();
            let value = value.trim();
            match key.as_str() {
                "name" => name = value.to_string(),
                "description" => description = value.to_string(),
                "schema" => schema_str = value.to_string(),
                "approval" => approval_str = value.to_string(),
                _ => {}
            }
        }
    }

    let input_schema = if schema_str.is_empty() {
        // Default: accept any object payload
        serde_json::json!({"type": "object"})
    } else {
        serde_json::from_str(&schema_str).unwrap_or_else(|_| serde_json::json!({"type": "object"}))
    };

    let approval = match approval_str.to_lowercase().as_str() {
        "auto" => ApprovalRequirement::Auto,
        "required" => ApprovalRequirement::Required,
        _ => ApprovalRequirement::Suggest,
    };

    PluginMetadata {
        name: if name.is_empty() {
            "unnamed-plugin".to_string()
        } else {
            name
        },
        description: if description.is_empty() {
            "User-provided plugin tool".to_string()
        } else {
            description
        },
        input_schema,
        approval,
    }
}

/// Read the first 4 KB of a file and parse its frontmatter.
fn read_script_metadata(path: &Path) -> Option<PluginMetadata> {
    let mut file = std::fs::File::open(path).ok()?;
    let content = read_prefix_to_string(&mut file, 4096)?;
    let meta = parse_frontmatter(&content);
    // Require at least the `name` field to consider it a valid plugin.
    if meta.name == "unnamed-plugin" {
        return None;
    }
    Some(meta)
}

// ---------------------------------------------------------------------------
// Directory scanning
// ---------------------------------------------------------------------------

/// Scan a directory for plugin script files with frontmatter headers.
///
/// Files are considered eligible when:
/// - They are regular files (not directories, not symlinks)
/// - They don't start with `.` (hidden files)
/// - They are not `README.md`
/// - Their first 20 lines contain `# name:` frontmatter
pub fn scan_plugin_dir(dir: &Path) -> Vec<(PathBuf, PluginMetadata)> {
    let mut results = Vec::new();

    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => {
            tracing::warn!("Failed to read plugin directory {}: {e}", dir.display());
            return results;
        }
    };

    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let path = entry.path();

        // Skip directories and hidden files
        if path.is_dir() {
            continue;
        }
        if let Some(name) = path.file_name().and_then(|n| n.to_str())
            && (name.starts_with('.') || name == "README.md")
        {
            continue;
        }

        // Try to parse frontmatter
        if let Some(meta) = read_script_metadata(&path) {
            results.push((path, meta));
        }
    }

    results
}

/// Load all plugin tools from a directory. Each eligible script becomes
/// a registered `ScriptPluginTool`.
pub fn load_plugin_tools(plugin_dir: &Path) -> Vec<Arc<dyn ToolSpec>> {
    let discovered = scan_plugin_dir(plugin_dir);
    let mut tools: Vec<Arc<dyn ToolSpec>> = Vec::with_capacity(discovered.len());

    for (path, meta) in discovered {
        tracing::info!(
            "Discovered plugin tool '{}' at {}",
            meta.name,
            path.display()
        );
        tools.push(Arc::new(ScriptPluginTool {
            metadata: meta,
            script_path: path,
            args: Vec::new(),
        }));
    }

    tools
}

/// Create a single tool from a `ToolOverride` config entry.
///
/// Returns `None` for `Disabled` (the caller handles removal separately).
pub fn tool_from_override(
    tool_name: &str,
    override_cfg: &ToolOverride,
    plugin_dir: &Path,
) -> Option<Arc<dyn ToolSpec>> {
    match override_cfg {
        ToolOverride::Disabled => None,
        ToolOverride::Script { path, args } => {
            let script_path = if Path::new(path).is_absolute() {
                PathBuf::from(path)
            } else {
                // Relative paths resolve relative to the plugin directory.
                plugin_dir.join(path)
            };

            if !script_path.exists() {
                tracing::warn!(
                    "Override script for '{}' not found at {}",
                    tool_name,
                    script_path.display()
                );
                return None;
            }

            // Read the script's own frontmatter for metadata, or provide
            // defaults if it has none.
            let meta = read_script_metadata(&script_path).unwrap_or_else(|| PluginMetadata {
                name: tool_name.to_string(),
                description: format!("Override for built-in tool '{tool_name}'"),
                input_schema: serde_json::json!({"type": "object"}),
                approval: ApprovalRequirement::Suggest,
            });

            Some(Arc::new(ScriptPluginTool {
                metadata: meta,
                script_path,
                args: args.clone().unwrap_or_default(),
            }) as Arc<dyn ToolSpec>)
        }
        ToolOverride::Command { command, args } => {
            // Build a description that includes the command.
            let description = format!("Override for '{tool_name}' — runs: {command}");
            let cmd_args = args.clone().unwrap_or_default();

            Some(Arc::new(CommandPluginTool {
                name: tool_name.to_string(),
                description,
                input_schema: serde_json::json!({"type": "object"}),
                command: command.clone(),
                args: cmd_args,
                approval: ApprovalRequirement::Suggest,
            }) as Arc<dyn ToolSpec>)
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {}
