//! Offline evaluation harness for exercising representative tool loops.
//!
//! This module is intentionally self-contained so it can be wired into a CLI
//! command later without calling the network or any LLM endpoints.

use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use ignore::WalkBuilder;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tempfile::TempDir;

use crate::llm_client::LlmClient;
use crate::llm_client::mock::MockLlmClient;
use crate::models::{ContentBlockStart, MessageRequest, StreamEvent};
use crate::sandbox::backend::{SandboxBackend, SandboxOutput};
use crate::tools::registry::ToolRegistryBuilder;
use crate::tools::spec::{ToolContext, ToolError};

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EvalShellPlatform {
    Windows,
    Unix,
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
struct EvalShellInvocation {
    program: &'static str,
    args: Vec<String>,
    raw_payload_on_windows: bool,
}

/// Representative tool steps covered by the evaluation harness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum ScenarioStepKind {
    List,
    Read,
    Search,
    Edit,
    ApplyPatch,
    ExecShell,
    /// A tool executed through the real `ToolRegistry` driven by the mock LLM
    /// client (e.g. `gadget_chain_trace`, `run_poc`, `hypothesis`). The actual
    /// tool name is carried on the [`EvalStep::tool_name`] field.
    AgentTool,
}

impl ScenarioStepKind {
    /// Tool name associated with this step.
    pub fn tool_name(self) -> &'static str {
        match self {
            ScenarioStepKind::List => "list_dir",
            ScenarioStepKind::Read => "read_file",
            ScenarioStepKind::Search => "search",
            ScenarioStepKind::Edit => "edit_file",
            ScenarioStepKind::ApplyPatch => "apply_patch",
            ScenarioStepKind::ExecShell => "exec_shell",
            ScenarioStepKind::AgentTool => "agent_tool",
        }
    }

    /// Parse a step kind from CLI-friendly strings.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_lowercase().as_str() {
            "list" | "list_dir" => Some(Self::List),
            "read" | "read_file" => Some(Self::Read),
            "search" | "grep" | "grep_files" => Some(Self::Search),
            "edit" | "edit_file" => Some(Self::Edit),
            "patch" | "apply_patch" => Some(Self::ApplyPatch),
            "shell" | "exec_shell" | "exec" => Some(Self::ExecShell),
            "agent" | "agent_tool" => Some(Self::AgentTool),
            _ => None,
        }
    }
}

/// Aggregate statistics for a single tool kind.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ToolStats {
    pub invocations: usize,
    pub errors: usize,
    pub total_duration: Duration,
}

/// Top-level metrics produced by an evaluation run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EvalMetrics {
    pub success: bool,
    pub tool_errors: usize,
    pub steps: usize,
    pub duration: Duration,
    pub per_tool: BTreeMap<ScenarioStepKind, ToolStats>,
}

/// One tool invocation recorded by the harness.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EvalStep {
    pub kind: ScenarioStepKind,
    pub tool_name: &'static str,
    pub success: bool,
    pub duration: Duration,
    pub error: Option<String>,
    pub output: Option<String>,
}

/// Summary of the generated temporary workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorkspaceSummary {
    pub root: PathBuf,
    pub file_count: usize,
    pub files: Vec<PathBuf>,
}

/// Configuration for the offline evaluation harness.
#[derive(Debug, Clone)]
pub struct EvalHarnessConfig {
    /// Human-readable scenario name for reporting.
    pub scenario_name: String,
    /// If set, the harness will intentionally fail this step to test metrics.
    pub fail_step: Option<ScenarioStepKind>,
    /// Shell command executed during the `exec_shell` step.
    pub shell_command: String,
    /// Token that must appear in shell output for validation.
    pub shell_expect_token: String,
    /// Maximum characters stored for step output summaries.
    pub max_output_chars: usize,
    /// When set, every step is appended as a JSON Lines fixture to a file
    /// inside this directory. The fixture file is named after the scenario
    /// (e.g. `offline-tool-loop.jsonl`). Each line follows the schema:
    /// `{ "request": <step descriptor>, "response_events": [<events>] }`.
    /// The mock LLM client (`crate::llm_client::mock`) can replay these
    /// fixtures for deterministic offline tests. See
    /// `crates/tui/tests/README.md` for the full record/replay flow.
    pub record_dir: Option<PathBuf>,
}

impl Default for EvalHarnessConfig {
    fn default() -> Self {
        let shell_command = if cfg!(windows) {
            "echo eval-harness".to_string()
        } else {
            "printf eval-harness".to_string()
        };
        Self {
            scenario_name: "offline-tool-loop".to_string(),
            fail_step: None,
            shell_command,
            shell_expect_token: "eval-harness".to_string(),
            max_output_chars: 240,
            record_dir: None,
        }
    }
}

/// Offline harness that exercises representative tool loops in a temp workspace.
#[derive(Debug, Clone)]
pub struct EvalHarness {
    config: EvalHarnessConfig,
}

impl EvalHarness {
    /// Create a new harness with the provided configuration.
    pub fn new(config: EvalHarnessConfig) -> Self {
        Self { config }
    }

    /// Execute the offline evaluation scenario and return detailed results.
    ///
    /// This drives the **real** offline tool loop: a `ToolRegistry` built from
    /// the vulnerability-hunting tools (`hypothesis`, `gadget_chain_trace`,
    /// `run_poc`) is exercised through their real `ToolSpec::execute` methods,
    /// while the LLM role is fulfilled by a deterministic [`MockLlmClient`] that
    /// replays a scripted turn. No network or live model is involved.
    pub fn run(&self) -> Result<EvalRun> {
        let started_at = Instant::now();
        let workspace = tempfile::Builder::new()
            .prefix("deepseek-eval-")
            .tempdir()
            .context("failed to create evaluation workspace")?;

        let _seed = seed_workspace(workspace.path())?;

        let mut steps = Vec::new();
        let mut per_tool: BTreeMap<ScenarioStepKind, ToolStats> = BTreeMap::new();
        let mut agent_tool_calls: Vec<String> = Vec::new();
        let mut realized = false;
        let mut tool_errors = 0usize;

        // Drive the loop: pop each queued turn, stream its events, extract the
        // tool-use blocks, and execute them through the REAL registry. The
        // registry and mock client are async, so we run them on a runtime while
        // keeping `run` synchronous (stable API). Reuse the ambient Tokio
        // runtime when present (e.g. `main`/`run_eval`), otherwise spin up a
        // dedicated current-thread runtime for offline/test use.
        let drive = async {
            // Build a real tool context. A self-contained in-memory sandbox
            // backend is injected so `run_poc` can execute its candidate PoC
            // offline and report whether the vulnerability was realized
            // (fail-closed otherwise).
            let mut context = ToolContext::new(workspace.path());
            context.sandbox_backend = Some(Arc::new(InMemorySandboxBackend));

            let registry = ToolRegistryBuilder::new()
                .with_hypothesis_tools()
                .with_gadget_chain_tools()
                .with_run_poc_tools()
                .build(context);

            // Scripted agent turn replayed by the mock LLM client:
            //   1. call gadget_chain_trace
            //   2. call run_poc
            let mock = MockLlmClient::new();
            mock.push_tool_call(
                "gadget_chain_trace",
                json!({
                    "sink": "InitialContext.lookup",
                    "present_gadgets": ["c3p0-jndi", "jndi-lookup"]
                }),
            );
            mock.push_tool_call(
                "run_poc",
                json!({
                    "command": "printf POC_REALIZED",
                    "expect": "POC_REALIZED"
                }),
            );

            while mock.pending() > 0 {
                let stream = mock.create_message_stream(self.build_request()).await?;
                let tool_calls = collect_tool_calls(stream).await?;
                if tool_calls.is_empty() {
                    break;
                }
                for (name, input) in tool_calls {
                    let started = Instant::now();
                    let stat = per_tool.entry(ScenarioStepKind::AgentTool).or_default();
                    stat.invocations += 1;

                    let outcome = registry.execute_full(&name, input).await;
                    let duration = started.elapsed();
                    stat.total_duration += duration;

                    match outcome {
                        Ok(result) => {
                            if name == "run_poc" {
                                // `run_poc` serializes its outcome into
                                // `content` (JSON) rather than `metadata`.
                                if let Ok(value) =
                                    serde_json::from_str::<serde_json::Value>(&result.content)
                                    && value.get("realized").and_then(|v| v.as_bool()) == Some(true)
                                {
                                    realized = true;
                                }
                            }
                            agent_tool_calls.push(name.clone());
                            steps.push(EvalStep {
                                kind: ScenarioStepKind::AgentTool,
                                tool_name: name_leak(&name),
                                success: result.success,
                                duration,
                                error: if result.success {
                                    None
                                } else {
                                    Some(result.content.clone())
                                },
                                output: Some(truncate_output(
                                    &result.content,
                                    self.config.max_output_chars,
                                )),
                            });
                            if !result.success {
                                stat.errors += 1;
                                tool_errors += 1;
                            }
                        }
                        Err(err) => {
                            stat.errors += 1;
                            tool_errors += 1;
                            steps.push(EvalStep {
                                kind: ScenarioStepKind::AgentTool,
                                tool_name: name_leak(&name),
                                success: false,
                                duration,
                                error: Some(err.to_string()),
                                output: None,
                            });
                        }
                    }
                }
            }
            Ok::<(), anyhow::Error>(())
        };

        match tokio::runtime::Handle::try_current() {
            Ok(handle) => handle.block_on(drive)?,
            Err(_) => {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .context("failed to start eval runtime")?;
                runtime.block_on(drive)?;
            }
        }

        let duration = started_at.elapsed();

        let workspace_summary = summarize_workspace(workspace.path(), None)?;

        let validation_success = validate_real_loop(&agent_tool_calls, realized);

        let success = tool_errors == 0 && validation_success;

        let metrics = EvalMetrics {
            success,
            tool_errors,
            steps: steps.len(),
            duration,
            per_tool,
        };

        Ok(EvalRun {
            scenario_name: self.config.scenario_name.clone(),
            workspace,
            workspace_summary,
            metrics,
            steps,
            agent_tool_calls,
        })
    }

    /// Build the (unused-by-mock) `MessageRequest` passed to the mock client.
    fn build_request(&self) -> MessageRequest {
        MessageRequest {
            model: "mock".to_string(),
            messages: Vec::new(),
            max_tokens: 1024,
            system: None,
            tools: None,
            tool_choice: None,
            metadata: None,
            thinking: None,
            reasoning_effort: None,
            stream: Some(true),
            temperature: None,
            top_p: None,
            response_format: None,
        }
    }
}

/// Leak a `String` tool name into a `&'static str` for the `EvalStep.tool_name`
/// field. The harness owns the names for the lifetime of the run; see
/// `EvalRun::agent_tool_calls` for the owned copy used by tests.
fn name_leak(name: &str) -> &'static str {
    Box::leak(name.to_string().into_boxed_str())
}

/// Collect `(tool_name, input)` pairs from a streamed assistant turn.
async fn collect_tool_calls(
    mut stream: crate::llm_client::StreamEventBox,
) -> Result<Vec<(String, serde_json::Value)>> {
    use futures_util::StreamExt;
    let mut calls = Vec::new();
    while let Some(item) = stream.next().await {
        let event = item?;
        if let StreamEvent::ContentBlockStart {
            content_block: ContentBlockStart::ToolUse { name, input, .. },
            ..
        } = event
        {
            calls.push((name, input));
        }
    }
    Ok(calls)
}

/// Validate the real tool loop: the scripted tools must have executed and the
/// PoC must have been realized.
fn validate_real_loop(agent_tool_calls: &[String], realized: bool) -> bool {
    let called_gadget = agent_tool_calls.iter().any(|t| t == "gadget_chain_trace");
    let called_poc = agent_tool_calls.iter().any(|t| t == "run_poc");
    called_gadget && called_poc && realized
}

// === Fixture record/replay format ===========================================
//
// The `--record` flag writes one JSON object per line to a `.jsonl` file:
//
//     { "request": { "step": "list_dir", "kind": "List" },
//       "response_events": [{ "type": "ok", "output": "…" }] }
//
// The mock LLM client replays these fixtures via
// `MockLlmClient::push_message_response` (or the streaming variant) by mapping
// each `response_events` array onto a canned `Vec<StreamEvent>`.
//
// This format is intentionally minimal — additional fields (timing, model,
// usage) can be added without breaking older fixtures because each line is a
// self-contained JSON object.

/// Schema for one line of a `--record` JSONL fixture file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixtureRecord {
    /// Step descriptor (`{ step, kind }`).
    pub request: serde_json::Value,
    /// One or more synthetic response events.
    pub response_events: Vec<serde_json::Value>,
}

impl FixtureRecord {
    fn ok(kind: ScenarioStepKind, output: &str) -> Self {
        Self {
            request: serde_json::json!({
                "step": kind.tool_name(),
                "kind": format!("{kind:?}"),
            }),
            response_events: vec![serde_json::json!({
                "type": "ok",
                "output": output,
            })],
        }
    }

    fn err(kind: ScenarioStepKind, error: &str) -> Self {
        Self {
            request: serde_json::json!({
                "step": kind.tool_name(),
                "kind": format!("{kind:?}"),
            }),
            response_events: vec![serde_json::json!({
                "type": "error",
                "error": error,
            })],
        }
    }
}

/// Append one fixture record to `<dir>/<scenario>.jsonl` (creating dir + file
/// if missing). Best-effort: I/O errors are returned but generally ignored by
/// the harness so a recording failure does not mask the run's primary result.
pub fn record_fixture(dir: &Path, scenario_name: &str, record: FixtureRecord) -> Result<PathBuf> {
    fs::create_dir_all(dir)
        .with_context(|| format!("failed to create fixture dir: {}", dir.display()))?;
    let safe_scenario = scenario_name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>();
    let path = dir.join(format!("{safe_scenario}.jsonl"));
    let line = serde_json::to_string(&record).context("failed to serialize fixture record")?;

    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("failed to open fixture file: {}", path.display()))?;
    writeln!(file, "{line}")
        .with_context(|| format!("failed to write fixture line to {}", path.display()))?;
    Ok(path)
}

impl Default for EvalHarness {
    fn default() -> Self {
        Self::new(EvalHarnessConfig::default())
    }
}

/// Result of running the evaluation harness.
#[derive(Debug)]
pub struct EvalRun {
    pub scenario_name: String,
    workspace: TempDir,
    pub workspace_summary: WorkspaceSummary,
    pub metrics: EvalMetrics,
    pub steps: Vec<EvalStep>,
    /// Names of the real tools executed through the `ToolRegistry` while the
    /// harness was driven by the mock LLM client. Empty for the legacy
    /// file-step path. Used by tests/CI to assert the real tool loop ran.
    pub agent_tool_calls: Vec<String>,
}

impl EvalRun {
    /// Get the root of the temporary workspace.
    pub fn workspace_root(&self) -> &Path {
        self.workspace.path()
    }

    /// Convert the run into a serializable report for CLI output.
    pub fn to_report(&self) -> EvalReport {
        EvalReport {
            scenario_name: self.scenario_name.clone(),
            workspace_root: self.workspace_root().to_path_buf(),
            workspace_summary: self.workspace_summary.clone(),
            metrics: self.metrics.clone(),
            steps: self.steps.clone(),
            agent_tool_calls: self.agent_tool_calls.clone(),
        }
    }
}

/// Serializable report derived from an `EvalRun`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct EvalReport {
    pub scenario_name: String,
    pub workspace_root: PathBuf,
    pub workspace_summary: WorkspaceSummary,
    pub metrics: EvalMetrics,
    pub steps: Vec<EvalStep>,
    /// Names of the real tools executed through the `ToolRegistry`.
    pub agent_tool_calls: Vec<String>,
}

#[derive(Debug, Clone)]
struct SeedWorkspace {
    notes_path: PathBuf,
}

fn seed_workspace(root: &Path) -> Result<SeedWorkspace> {
    let src_dir = root.join("src");
    fs::create_dir_all(&src_dir)
        .with_context(|| format!("failed to create seed directory: {}", src_dir.display()))?;

    let readme_path = root.join("README.md");
    fs::write(
        &readme_path,
        "# Eval Harness Workspace\n\nThis workspace is offline.\n",
    )
    .with_context(|| format!("failed to write {}", readme_path.display()))?;

    let notes_path = root.join("notes.txt");
    fs::write(
        &notes_path,
        "# Eval Harness\nstatus = \"draft\"\ntodo: offline metrics\n",
    )
    .with_context(|| format!("failed to write {}", notes_path.display()))?;

    let lib_path = src_dir.join("lib.rs");
    fs::write(
        &lib_path,
        "pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n",
    )
    .with_context(|| format!("failed to write {}", lib_path.display()))?;

    Ok(SeedWorkspace { notes_path })
}

fn summarize_workspace(root: &Path, list_output: Option<&str>) -> Result<WorkspaceSummary> {
    let mut files = Vec::new();

    let walker = WalkBuilder::new(root)
        .hidden(false)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .build();

    for entry in walker {
        let entry = entry.with_context(|| format!("failed to walk {}", root.display()))?;
        if entry.file_type().is_some_and(|t| t.is_file()) {
            files.push(entry.into_path());
        }
    }

    if files.is_empty()
        && let Some(output) = list_output
        && !output.trim().is_empty()
    {
        return Err(anyhow!(
            "workspace appears empty after list_dir: {}",
            output.trim()
        ));
    }

    files.sort();

    Ok(WorkspaceSummary {
        root: root.to_path_buf(),
        file_count: files.len(),
        files,
    })
}

/// Self-contained sandbox backend for offline evaluation.
///
/// It executes the candidate PoC command locally (so the real `run_poc`
/// `ToolSpec::execute` path runs end-to-end) and returns combined output. This
/// is intentionally not a security boundary — it exists so the eval harness can
/// deterministically verify the tool loop without an external sandbox service.
struct InMemorySandboxBackend;

#[async_trait]
impl SandboxBackend for InMemorySandboxBackend {
    async fn exec(&self, cmd: &str, _env: &HashMap<String, String>) -> Result<SandboxOutput> {
        use std::process::Command;
        let output = Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .output()
            .map_err(|e| ToolError::execution_failed(format!("sandbox exec failed: {e}")))?;
        Ok(SandboxOutput {
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            exit_code: output.status.code().unwrap_or(-1),
        })
    }
}

fn truncate_output(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }

    let truncated: String = value.chars().take(max_chars).collect();
    format!("{truncated}...")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// End-to-end: `EvalHarness::run` drives the REAL `ToolRegistry`
    /// (`gadget_chain_trace` + `run_poc`) through their real `ToolSpec::execute`
    /// methods, with the LLM role fulfilled by `MockLlmClient`. The harness must
    /// report that both tools actually executed and that `run_poc` realized
    /// the vulnerability.
    #[test]
    fn harness_drives_real_tool_loop_via_mock() {
        let config = EvalHarnessConfig {
            scenario_name: "vuln-hunt-real-loop".to_string(),
            ..EvalHarnessConfig::default()
        };
        let harness = EvalHarness::new(config);
        let run = harness.run().expect("harness run ok");

        // Both real tools must have been executed (not the old inline stubs).
        assert!(
            run.agent_tool_calls
                .iter()
                .any(|t| t == "gadget_chain_trace"),
            "gadget_chain_trace should have been executed through the real registry; got {:?}",
            run.agent_tool_calls
        );
        assert!(
            run.agent_tool_calls.iter().any(|t| t == "run_poc"),
            "run_poc should have been executed through the real registry; got {:?}",
            run.agent_tool_calls
        );

        // run_poc must have run its candidate PoC in the injected backend and
        // reported the vulnerability as realized.
        assert!(run.metrics.success, "harness should report success");
        assert_eq!(run.metrics.tool_errors, 0, "no tool errors expected");

        // The recorded steps must surface the real tool names.
        let names: Vec<&str> = run.steps.iter().map(|s| s.tool_name).collect();
        assert!(names.iter().any(|n| *n == "gadget_chain_trace"));
        assert!(names.iter().any(|n| *n == "run_poc"));
    }
}
