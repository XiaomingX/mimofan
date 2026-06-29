//! # 状态机模块
//!
//! 本模块提供项目中所有状态机的索引、文档和验证工具。
//! 实际状态枚举保留在原模块中，避免大规模迁移风险。
//!
//! ## 状态机清单
//!
//! | 状态机 | 源文件 | 状态数 | 说明 |
//! |--------|--------|--------|------|
//! | `GoalRunStatus` | `goal_loop.rs` | 3 | 目标运行状态 |
//! | `StopReason` | `goal_loop.rs` | 5 | 循环停止原因 |
//! | `ConnectionState` | `mcp.rs` | 3 | MCP 连接状态 |
//! | `SubAgentStatus` | `tools/subagent/mod.rs` | 6 | 子代理执行状态 |
//! | `AgentWorkerStatus` | `tools/subagent/mod.rs` | 10 | Agent Worker 状态 |
//! | `ApprovalMode` | `tui/approval.rs` | 3 | 审批模式 |
//!
//! ## 设计原则
//!
//! 1. **枚举保留在原模块** — 避免破坏现有 `use` 语句
//! 2. **本模块提供索引** — 便于查找和理解状态机
//! 3. **验证函数可选** — 需要时在原模块中实现

/// 状态机元数据
#[derive(Debug, Clone)]
pub struct StateMachineMetadata {
    /// 状态机名称
    pub name: &'static str,
    /// 所在文件
    pub file: &'static str,
    /// 状态数量
    pub state_count: usize,
    /// 说明
    pub description: &'static str,
    /// 主要状态列表
    pub states: &'static [&'static str],
}

/// 获取所有状态机元数据
pub fn all_state_machine_metadata() -> Vec<StateMachineMetadata> {
    vec![
        StateMachineMetadata {
            name: "GoalRunStatus",
            file: "goal_loop.rs",
            state_count: 3,
            description: "目标运行状态",
            states: &["Active", "Completed", "Blocked"],
        },
        StateMachineMetadata {
            name: "StopReason",
            file: "goal_loop.rs",
            state_count: 5,
            description: "循环停止原因",
            states: &["Completed", "Blocked", "TokenBudget", "TimeBudget", "ContinuationLimit"],
        },
        StateMachineMetadata {
            name: "ConnectionState",
            file: "mcp.rs",
            state_count: 3,
            description: "MCP 连接状态",
            states: &["Connecting", "Ready", "Disconnected"],
        },
        StateMachineMetadata {
            name: "SubAgentStatus",
            file: "tools/subagent/mod.rs",
            state_count: 6,
            description: "子代理执行状态",
            states: &["Running", "Completed", "Interrupted", "Failed", "Cancelled", "BudgetExhausted"],
        },
        StateMachineMetadata {
            name: "AgentWorkerStatus",
            file: "tools/subagent/mod.rs",
            state_count: 10,
            description: "Agent Worker 状态",
            states: &["Queued", "Starting", "Running", "WaitingForUser", "ModelWait", "RunningTool", "Completed", "Failed", "Cancelled", "Interrupted"],
        },
        StateMachineMetadata {
            name: "ApprovalMode",
            file: "tui/approval.rs",
            state_count: 3,
            description: "审批模式",
            states: &["Auto", "Suggest", "Never"],
        },
    ]
}

/// 按名称查找状态机
pub fn find_state_machine(name: &str) -> Option<StateMachineMetadata> {
    all_state_machine_metadata().into_iter().find(|sm| sm.name == name)
}

/// 按文件查找状态机
pub fn state_machines_in_file(file: &str) -> Vec<StateMachineMetadata> {
    all_state_machine_metadata()
        .into_iter()
        .filter(|sm| sm.file == file)
        .collect()
}
