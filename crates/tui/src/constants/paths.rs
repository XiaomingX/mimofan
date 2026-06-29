//! 路径相关常量
//!
//! 文件名、目录名、相对路径等常量定义。

/// 中继文件相对路径（相对于用户主目录）
pub const HANDOFF_RELATIVE_PATH: &str = ".mimo/handoff.md";

/// 旧版中继文件相对路径（向后兼容）
pub const LEGACY_HANDOFF_RELATIVE_PATH: &str = ".deepseek/handoff.md";

/// 信任文件名
pub const TRUST_FILE_NAME: &str = "workspace-trust.json";

/// 技能状态文件名
pub const STATE_FILE_NAME: &str = "skills_state.toml";

/// 历史文件名
pub const HISTORY_FILE_NAME: &str = "composer_history.txt";

/// 制品目录名
pub const ARTIFACTS_DIR_NAME: &str = "artifacts";

/// 弃用的 WHALE 文件名
pub const DEPRECATED_WHALE_FILENAME: &str = "WHALE.md";

/// 配置文件名
pub const CONFIG_FILE_NAME: &str = "config.toml";

/// 权限文件名
pub const PERMISSIONS_FILE_NAME: &str = "permissions.toml";

/// 应用目录名
pub const APP_DIR: &str = ".mimo";

/// 旧版应用目录名（向后兼容）
pub const PREVIOUS_APP_DIR: &str = ".mimofan";

/// 旧版应用目录名（向后兼容）
pub const LEGACY_APP_DIR: &str = ".deepseek";
