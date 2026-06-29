//! Token 相关常量
//!
//! 上下文窗口、Token 预算等常量定义。

/// 上下文余量 Token 数
pub const CONTEXT_HEADROOM_TOKENS: u64 = 1_024;

/// 最小输入预算 Token 数
pub const MIN_INPUT_BUDGET_TOKENS: u64 = 1_024;
