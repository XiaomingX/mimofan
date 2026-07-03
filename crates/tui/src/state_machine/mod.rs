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

