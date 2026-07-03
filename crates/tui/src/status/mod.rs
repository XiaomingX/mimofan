//! 统一状态码管理模块
//!
//! 将分散在各文件的状态码枚举收口到此处，便于查找和维护。
//!
//! ## 状态码分布索引
//!
//! ### 轮次相关状态
//! - `RuntimeTurnStatus` — `runtime_threads.rs:90` — 运行时轮次状态
//! - `TurnItemLifecycleStatus` — `runtime_threads.rs:115` — 轮次项生命周期状态
//! - `TurnOutcomeStatus` — `core/events.rs:19` — 轮次结果状态
//! - `AgentRebindStatus` — `runtime_threads.rs:3800` — Agent 重绑定状态
//!
//! ### Agent 相关状态
//! - `SubAgentStatus` — `tools/subagent/mod.rs:529` — 子代理状态
//! - `AgentWorkerStatus` — `tools/subagent/mod.rs:598` — Agent Worker 状态
//! - `AutomationStatus` — `automation_manager.rs:40` — 自动化状态
//! - `AutomationRunStatus` — `automation_manager.rs:47` — 自动化运行状态
//! - `GoalRunStatus` — `goal_loop.rs:22` — 目标运行状态
//!
//! ### 任务相关状态
//! - `TaskStatus` — `task_manager.rs:45` — 任务状态
//! - `JobStatus` — `core/job.rs:15` — 任务状态
//! - `StepStatus` — `tools/plan.rs:20` — 步骤状态
//! - `GoalStatus` — `tools/goal.rs:49` — 目标状态
//! - `TodoStatus` — `tools/todo.rs:19` — 待办状态
//!
//! ### 连接相关状态
//! - `ConnectionState` — `client.rs:187` — 连接状态
//! - `ConnectionState` — `mcp.rs:389` — MCP 连接状态
//! - `McpWriteStatus` — `mcp.rs:2715` — MCP 写入状态
//! - `ShellStatus` — `tools/shell.rs:53` — Shell 状态
//!
//! ### UI 相关状态
//! - `StatusIndicatorValue` — `config_ui.rs:267` — 状态指示器值
//! - `StatusItemValue` — `config_ui.rs:283` — 状态项值
//! - `StatusItem` — `config.rs:1409` — 状态项
//! - `ToolStatus` — `tui/history.rs:625` — 工具状态
//! - `TranslationStatus` — `tui/translation.rs:106` — 翻译状态
//!
//! ### 其他状态
//! - `CatalogStatus` — `config/catalog.rs:256` — 目录刷新状态
//! - `GateStatus` — `tools/verifier.rs:153` — 门控状态
//! - `WriteStatus` — `main.rs:1785` — 写入状态
//! - `McpServerDoctorStatus` — `main.rs:5296` — MCP 服务器诊断状态
//! - `RetryState` — `retry_status.rs:34` — 重试状态
//! - `SupportState` — `model_profile.rs:16` — 支持状态
//! - `ApprovalCacheStatus` — `tools/approval_cache.rs:56` — 审批缓存状态
