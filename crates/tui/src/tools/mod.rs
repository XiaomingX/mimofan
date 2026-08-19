//! Tool system modules and re-exports.

// NOTE: `print_stdout` / `print_stderr` are set to `allow` at the workspace
// lint level (see root `Cargo.toml`) so the release pipeline's `-D warnings`
// does not fail on intentional `println!`/`eprintln!` sinks (e.g. the
// stdout/stderr notify sinks). Tools inside the TUI alt-screen runtime should
// still prefer `tracing::*` to avoid leaking into ratatua's diff buffer.

/// #847：模型路由 advisor（Advisor::advise 纯函数 + AdvisorTool 薄 ToolSpec 包装）。
pub mod advisor;
pub mod apply_patch;
pub mod apply_patch_claude;
pub mod approval_cache;
pub mod arg_repair;
pub mod ast_query;
pub mod attack_surface_tool;
pub mod auto_gadget;
pub mod automation;
pub mod browser;
pub mod call_graph;
pub mod cargo_failure_summary;
pub mod dev_server_readiness;
pub mod diagnostics;
pub mod diff_format;
pub mod dynamic;
/// #850：结构化事件流（jsonl）+ replay。模块声明在此；工具注册（registry/engine）延后，本文件不接线。
pub mod event_stream;
pub mod file;
pub mod file_search;
pub mod finance;
pub mod gadget_chain;
/// #850：EventStreamTool（ReadOnly，实现 ToolSpec，注册延后）。
pub mod replay;
/// #639：统一 VFS 抽象（文件工具 IO 出入口）。
pub mod vfs;

/// #854：工具级权限策略（按 ToolCapability 裁决 deny_capability / 限网络）。模块声明在此；dispatch 接线延后，本文件不改动 registry 逻辑。
pub mod capability_policy;
pub mod codebase_search;
pub mod create_sub_session;
pub mod fetch_url;
pub mod fim;
pub mod git;
pub mod git_history;
pub mod github;
pub mod goal;
pub mod handle;
/// #803：Hypothesis/Evidence/Verdict 一等公民（vuln-hunt 推理严谨性轴）。
pub mod hypothesis;
pub mod image_ocr;
pub mod insights;
pub mod js_execution;
pub mod json_schema_terminator;
pub mod large_output_router;
pub mod lsp_symbols;
pub mod notebook_edit;
pub mod notify;
pub mod pandoc;
pub mod parallel;
pub mod plan;
pub mod plugin;
pub mod project;
pub mod protocol_check_tool;
/// #846：system prompt 冗余/矛盾/膨胀审计（PromptAuditTool 实现 ToolSpec，ReadOnly）。
pub mod prompt_audit;
pub mod record_artifact;
pub mod registry;
pub mod remember;
#[cfg(feature = "vector-memory")]
pub mod remember_vector;
#[cfg(feature = "vector-memory")]
pub mod session_search;
pub mod revert_turn;
pub mod review;
pub mod rlm;
pub mod run_in_disposable_sandbox;
pub mod run_poc;
#[cfg(feature = "vector-memory")]
pub mod schema_canonicalize;
pub mod schema_sanitize;
pub mod search;
pub mod security_audit;
/// #12-plan Phase 1：把 semgrep 辅助库（security_audit.rs）封装成 `security_audit` 工具，
/// 消除 security_auditor.md 人格「drive semgrep」指令不可用的文档谎言。
pub mod security_audit_tool;
pub mod shell;
mod shell_output;
pub mod skill;
pub mod spec;
pub mod speech;
pub mod subagent;
pub mod synthetic_output;
pub mod tasks;
pub mod test_runner;
pub mod todo;
pub mod tool_result_retrieval;
pub mod truncate;
pub mod unattended;
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
