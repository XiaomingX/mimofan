//! 统一常量管理模块
//!
//! 将分散在各文件的常量收口到此处，便于查找和维护。
//!
//! ## 常量分布索引
//!
//! ### 路径常量
//! - `HANDOFF_RELATIVE_PATH` — `prompts.rs:73`
//! - `LEGACY_HANDOFF_RELATIVE_PATH` — `prompts.rs:75`
//! - `TRUST_FILE_NAME` — `workspace_trust.rs:24`
//! - `STATE_FILE_NAME` — `skill_state.rs:25`
//! - `HISTORY_FILE_NAME` — `composer_history.rs:37`
//! - `ARTIFACTS_DIR_NAME` — `artifacts.rs:14`
//! - `DEPRECATED_WHALE_FILENAME` — `project_context.rs:44`
//! - `CONFIG_FILE_NAME` — `config.rs` (来自 config crate)
//! - `PERMISSIONS_FILE_NAME` — `config.rs` (来自 config crate)
//! - `APP_DIR` — `config.rs` (来自 config crate)
//!
//! ### 限制常量
//! - `MAX_SESSIONS` — `session_manager.rs:20`
//! - `CURRENT_SESSION_SCHEMA_VERSION` — `session_manager.rs:21`
//! - `CURRENT_QUEUE_SCHEMA_VERSION` — `session_manager.rs:22`
//! - `MAX_MEMORY_SIZE` — `memory.rs:34`
//! - `FILE_INDEX_MAX_ENTRIES` — `working_set.rs:429`
//! - `LOCAL_REFERENCE_SCAN_LIMIT` — `working_set.rs:546`
//! - `HOTBAR_SLOT_COUNT` — `config.rs` (来自 config crate)
//! - `DEFAULT_SPAWN_DEPTH` — `config.rs` (来自 config crate)
//! - `MAX_SPAWN_DEPTH_CEILING` — `config.rs` (来自 config crate)
//! - `DEFAULT_COMPACTION_TRIGGER_PERCENT` — `context_budget.rs:50`
//! - `CRITICAL_PRESSURE_PERCENT` — `context_budget.rs:53`
//! - `HIGH_PRESSURE_PERCENT` — `context_budget.rs:58`
//! - `MEDIUM_PRESSURE_PERCENT` — `context_budget.rs:62`
//! - `LEGACY_DEEPSEEK_CONTEXT_WINDOW_TOKENS` — `models.rs:7`
//! - `DEFAULT_COMPACTION_TOKEN_THRESHOLD` — `models.rs:16`
//! - `IGNORED_ROOT_DIRS` — `working_set.rs:1320`
//! - `INSTRUCTIONS_FILE_MAX_BYTES` — `prompts.rs:82`
//!
//! ### Token 常量
//! - `CONTEXT_HEADROOM_TOKENS` — `context_budget.rs:67`
//! - `MIN_INPUT_BUDGET_TOKENS` — `context_budget.rs:73`
//!
//! ### UI 常量
//! - `WHALE_BG_RGB` — `palette.rs:20`
//! - `WHALE_PANEL_RGB` — `palette.rs:21`
//! - `WHALE_ELEVATED_RGB` — `palette.rs:22`
//! - `WHALE_SELECTION_RGB` — `palette.rs:23`
//! - `WHALE_TEXT_BODY_RGB` — `palette.rs:24`
//! - `WHALE_TEXT_SOFT_RGB` — `palette.rs:25`
//! - `WHALE_TEXT_MUTED_RGB` — `palette.rs:26`
//! - `WHALE_TEXT_HINT_RGB` — `palette.rs:27`
//! - `WHALE_TEXT_DIM_RGB` — `palette.rs:29`
//! - `WHALE_ACCENT_PRIMARY_RGB` — `palette.rs:30`
//! - `WHALE_ACCENT_SECONDARY_RGB` — `palette.rs:31`
//! - `WHALE_ACCENT_ACTION_RGB` — `palette.rs:32`

pub mod limits;
pub mod paths;
pub mod tokens;
pub mod ui;

// Re-export all constants for backward compatibility
pub use limits::*;
pub use paths::*;
pub use tokens::*;
pub use ui::*;
