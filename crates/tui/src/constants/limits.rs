//! 限制相关常量
//!
//! 数量限制、大小限制、深度限制等常量定义。

/// 最大会话数
pub const MAX_SESSIONS: usize = 50;

/// 当前会话 schema 版本
pub const CURRENT_SESSION_SCHEMA_VERSION: u32 = 1;

/// 当前队列 schema 版本
pub const CURRENT_QUEUE_SCHEMA_VERSION: u32 = 1;

/// 最大内存大小（字节）
pub const MAX_MEMORY_SIZE: usize = 100 * 1024;

/// 文件索引最大条目数
pub const FILE_INDEX_MAX_ENTRIES: usize = 50_000;

/// 本地引用扫描限制
pub const LOCAL_REFERENCE_SCAN_LIMIT: usize = 4096;

/// 热键槽数量
pub const HOTBAR_SLOT_COUNT: u8 = 8;

/// 默认生成深度
pub const DEFAULT_SPAWN_DEPTH: u32 = 3;

/// 最大生成深度上限
pub const MAX_SPAWN_DEPTH_CEILING: u32 = 8;

/// 默认压缩触发百分比
pub const DEFAULT_COMPACTION_TRIGGER_PERCENT: f64 = 75.0;

/// 临界压力百分比
pub const CRITICAL_PRESSURE_PERCENT: f64 = 90.0;

/// 高压百分比
pub const HIGH_PRESSURE_PERCENT: f64 = DEFAULT_COMPACTION_TRIGGER_PERCENT;

/// 中压百分比
pub const MEDIUM_PRESSURE_PERCENT: f64 = 40.0;

/// 旧版 DeepSeek 上下文窗口 Token 数
pub const LEGACY_DEEPSEEK_CONTEXT_WINDOW_TOKENS: u32 = 128_000;

/// 默认压缩 Token 阈值
pub const DEFAULT_COMPACTION_TOKEN_THRESHOLD: usize = 102_400;

/// 忽略的根目录列表
pub const IGNORED_ROOT_DIRS: &[&str] = &["target", "node_modules", "dist", "build", ".git"];

/// 读取项目指令文件的最大字节数
pub const INSTRUCTIONS_FILE_MAX_BYTES: usize = 100 * 1024;
