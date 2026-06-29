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

// 状态码模块保持轻量，主要作为索引和文档
// 实际的状态码定义仍保留在原文件中
// 未来可以逐步迁移到此处

/// 状态码分类枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusCategory {
    /// 轮次相关
    Turn,
    /// Agent 相关
    Agent,
    /// 任务相关
    Task,
    /// 连接相关
    Connection,
    /// UI 相关
    Ui,
    /// 其他
    Other,
}

/// 状态码元数据
#[derive(Debug, Clone)]
pub struct StatusMetadata {
    /// 状态码名称
    pub name: &'static str,
    /// 所属分类
    pub category: StatusCategory,
    /// 定义位置（文件:行号）
    pub location: &'static str,
    /// 说明
    pub description: &'static str,
}

/// 获取所有状态码的元数据
pub fn all_status_metadata() -> Vec<StatusMetadata> {
    vec![
        // 轮次相关
        StatusMetadata {
            name: "RuntimeTurnStatus",
            category: StatusCategory::Turn,
            location: "runtime_threads.rs:90",
            description: "运行时轮次状态",
        },
        StatusMetadata {
            name: "TurnItemLifecycleStatus",
            category: StatusCategory::Turn,
            location: "runtime_threads.rs:115",
            description: "轮次项生命周期状态",
        },
        StatusMetadata {
            name: "TurnOutcomeStatus",
            category: StatusCategory::Turn,
            location: "core/events.rs:19",
            description: "轮次结果状态",
        },
        StatusMetadata {
            name: "AgentRebindStatus",
            category: StatusCategory::Turn,
            location: "runtime_threads.rs:3800",
            description: "Agent 重绑定状态",
        },
        // Agent 相关
        StatusMetadata {
            name: "SubAgentStatus",
            category: StatusCategory::Agent,
            location: "tools/subagent/mod.rs:529",
            description: "子代理状态",
        },
        StatusMetadata {
            name: "AgentWorkerStatus",
            category: StatusCategory::Agent,
            location: "tools/subagent/mod.rs:598",
            description: "Agent Worker 状态",
        },
        StatusMetadata {
            name: "AutomationStatus",
            category: StatusCategory::Agent,
            location: "automation_manager.rs:40",
            description: "自动化状态",
        },
        StatusMetadata {
            name: "AutomationRunStatus",
            category: StatusCategory::Agent,
            location: "automation_manager.rs:47",
            description: "自动化运行状态",
        },
        StatusMetadata {
            name: "GoalRunStatus",
            category: StatusCategory::Agent,
            location: "goal_loop.rs:22",
            description: "目标运行状态",
        },
        // 任务相关
        StatusMetadata {
            name: "TaskStatus",
            category: StatusCategory::Task,
            location: "task_manager.rs:45",
            description: "任务状态",
        },
        StatusMetadata {
            name: "JobStatus",
            category: StatusCategory::Task,
            location: "core/job.rs:15",
            description: "任务状态",
        },
        StatusMetadata {
            name: "StepStatus",
            category: StatusCategory::Task,
            location: "tools/plan.rs:20",
            description: "步骤状态",
        },
        StatusMetadata {
            name: "GoalStatus",
            category: StatusCategory::Task,
            location: "tools/goal.rs:49",
            description: "目标状态",
        },
        StatusMetadata {
            name: "TodoStatus",
            category: StatusCategory::Task,
            location: "tools/todo.rs:19",
            description: "待办状态",
        },
        // 连接相关
        StatusMetadata {
            name: "ConnectionState (client)",
            category: StatusCategory::Connection,
            location: "client.rs:187",
            description: "连接状态",
        },
        StatusMetadata {
            name: "ConnectionState (mcp)",
            category: StatusCategory::Connection,
            location: "mcp.rs:389",
            description: "MCP 连接状态",
        },
        StatusMetadata {
            name: "McpWriteStatus",
            category: StatusCategory::Connection,
            location: "mcp.rs:2715",
            description: "MCP 写入状态",
        },
        StatusMetadata {
            name: "ShellStatus",
            category: StatusCategory::Connection,
            location: "tools/shell.rs:53",
            description: "Shell 状态",
        },
        // UI 相关
        StatusMetadata {
            name: "StatusIndicatorValue",
            category: StatusCategory::Ui,
            location: "config_ui.rs:267",
            description: "状态指示器值",
        },
        StatusMetadata {
            name: "StatusItemValue",
            category: StatusCategory::Ui,
            location: "config_ui.rs:283",
            description: "状态项值",
        },
        StatusMetadata {
            name: "StatusItem",
            category: StatusCategory::Ui,
            location: "config.rs:1409",
            description: "状态项",
        },
        StatusMetadata {
            name: "ToolStatus",
            category: StatusCategory::Ui,
            location: "tui/history.rs:625",
            description: "工具状态",
        },
        StatusMetadata {
            name: "TranslationStatus",
            category: StatusCategory::Ui,
            location: "tui/translation.rs:106",
            description: "翻译状态",
        },
        // 其他
        StatusMetadata {
            name: "CatalogStatus",
            category: StatusCategory::Other,
            location: "config/catalog.rs:256",
            description: "目录刷新状态",
        },
        StatusMetadata {
            name: "GateStatus",
            category: StatusCategory::Other,
            location: "tools/verifier.rs:153",
            description: "门控状态",
        },
        StatusMetadata {
            name: "WriteStatus",
            category: StatusCategory::Other,
            location: "main.rs:1785",
            description: "写入状态",
        },
        StatusMetadata {
            name: "McpServerDoctorStatus",
            category: StatusCategory::Other,
            location: "main.rs:5296",
            description: "MCP 服务器诊断状态",
        },
        StatusMetadata {
            name: "RetryState",
            category: StatusCategory::Other,
            location: "retry_status.rs:34",
            description: "重试状态",
        },
        StatusMetadata {
            name: "SupportState",
            category: StatusCategory::Other,
            location: "model_profile.rs:16",
            description: "支持状态",
        },
        StatusMetadata {
            name: "ApprovalCacheStatus",
            category: StatusCategory::Other,
            location: "tools/approval_cache.rs:56",
            description: "审批缓存状态",
        },
    ]
}
