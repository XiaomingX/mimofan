//! Tool system modules and re-exports.

// Tools run inside the TUI alt-screen runtime. Raw `print!` / `eprintln!`
// inside this module tree leaks into ratatui's diff-renderer buffer and
// produces the "scroll demon" regression (#1085 / v0.8.27 follow-up).
// Route status/error reporting through `tracing::*` instead — the
// `runtime_log` subscriber captures it to `~/.mimofan/logs/`.
#![deny(clippy::print_stdout)]
#![deny(clippy::print_stderr)]

pub mod apply_patch;
pub mod approval_cache;
pub mod arg_repair;
/// #847：模型路由 advisor（Advisor::advise 纯函数 + AdvisorTool 薄 ToolSpec 包装）。
pub mod advisor;
pub mod ast_query;
pub mod call_graph;
pub mod gadget_chain;
pub mod automation;
pub mod browser;
pub mod cargo_failure_summary;
pub mod dev_server_readiness;
pub mod diagnostics;
/// #850：结构化事件流（jsonl）+ replay。模块声明在此；工具注册（registry/engine）延后，本文件不接线。
pub mod event_stream;
/// #850：EventStreamTool（ReadOnly，实现 ToolSpec，注册延后）。
pub mod replay;
pub mod diff_format;
pub mod dynamic;
pub mod file;
pub mod file_search;
pub mod finance;
/// #639：统一 VFS 抽象（文件工具 IO 出入口）。
pub mod vfs;

pub mod fetch_url;
pub mod fim;
pub mod git;
pub mod git_history;
pub mod github;
pub mod goal;
/// #803：Hypothesis/Evidence/Verdict 一等公民（vuln-hunt 推理严谨性轴）。
pub mod hypothesis;
pub mod handle;
pub mod image_ocr;
pub mod insights;
pub mod js_execution;
pub mod large_output_router;
pub mod lsp_symbols;
pub mod notebook_edit;
pub mod notify;
pub mod pandoc;
pub mod parallel;
pub mod plan;
pub mod plugin;
/// #846：system prompt 冗余/矛盾/膨胀审计（PromptAuditTool 实现 ToolSpec，ReadOnly）。
pub mod prompt_audit;
pub mod project;
pub mod registry;
pub mod remember;
pub mod record_artifact;
pub mod create_sub_session;
#[cfg(feature = "vector-memory")]
pub mod remember_vector;
pub mod revert_turn;
pub mod review;
pub mod rlm;
pub mod run_poc;
pub mod security_audit;
pub mod run_in_disposable_sandbox;
#[cfg(feature = "vector-memory")]
pub mod schema_canonicalize;
pub mod schema_sanitize;
pub mod search;
pub mod shell;
mod shell_output;
pub mod skill;
pub mod spec;
pub mod speech;
pub mod subagent;
pub mod synthetic_output;
pub mod json_schema_terminator;
pub mod tasks;
pub mod test_runner;
pub mod todo;
pub mod tool_result_retrieval;
pub mod unattended;
pub mod truncate;
pub mod user_input;
pub mod validate_data;
pub mod verifier;
pub mod web_run;
pub mod web_search;
pub mod workflow;
pub mod worktree;

pub use registry::{ToolRegistry, ToolRegistryBuilder};
pub use review::ReviewOutput;
pub use spec::ToolContext;
pub use user_input::UserInputResponse;
