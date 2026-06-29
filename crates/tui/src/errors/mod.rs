//! # 错误处理模块
//!
//! 本模块提供项目中所有错误类型的索引、文档和分类工具。
//! 实际错误类型保留在原模块中，使用 `error_taxonomy` 进行统一分类。
//!
//! ## 错误类型清单
//!
//! | 错误类型 | 源文件 | 说明 |
//! |----------|--------|------|
//! | `ErrorCategory` | `error_taxonomy.rs` | 错误分类（10 类） |
//! | `ErrorSeverity` | `error_taxonomy.rs` | 错误严重程度（4 级） |
//! | `ErrorEnvelope` | `error_taxonomy.rs` | 统一错误信封 |
//! | `LlmError` | `llm_client/mod.rs` | LLM 客户端错误 |
//! | `ToolError` | `tools/spec.rs` | 工具执行错误 |
//! | `ArgRepairError` | `tools/arg_repair.rs` | 参数修复错误 |
//! | `FimError` | `tools/fim.rs` | FIM 错误 |
//! | `ProjectContextError` | `project_context.rs` | 项目上下文错误 |
//! | `ApiKeyError` | `tui/app.rs` | API Key 错误 |
//!
//! ## 错误分类体系
//!
//! ```text
//! ErrorCategory
//! ├── Network        — 网络连接错误
//! ├── Authentication — 认证失败
//! ├── Authorization  — 授权失败
//! ├── RateLimit      — 速率限制
//! ├── Timeout        — 超时
//! ├── InvalidInput   — 无效输入
//! ├── Parse          — 解析错误
//! ├── Tool           — 工具错误
//! ├── State          — 状态错误
//! └── Internal       — 内部错误
//! ```

// 重导出 error_taxonomy 类型，保持向后兼容
pub use crate::error_taxonomy::{ErrorCategory, ErrorEnvelope, ErrorSeverity};

/// 错误元数据
#[derive(Debug, Clone)]
pub struct ErrorTypeMetadata {
    /// 错误类型名称
    pub name: &'static str,
    /// 所在文件
    pub file: &'static str,
    /// 说明
    pub description: &'static str,
    /// 主要变体
    pub variants: &'static [&'static str],
}

/// 获取所有错误类型元数据
pub fn all_error_type_metadata() -> Vec<ErrorTypeMetadata> {
    vec![
        ErrorTypeMetadata {
            name: "ErrorCategory",
            file: "error_taxonomy.rs",
            description: "错误分类枚举",
            variants: &["Network", "Authentication", "Authorization", "RateLimit", "Timeout", "InvalidInput", "Parse", "Tool", "State", "Internal"],
        },
        ErrorTypeMetadata {
            name: "ErrorSeverity",
            file: "error_taxonomy.rs",
            description: "错误严重程度",
            variants: &["Info", "Warning", "Error", "Critical"],
        },
        ErrorTypeMetadata {
            name: "LlmError",
            file: "llm_client/mod.rs",
            description: "LLM 客户端错误",
            variants: &["RateLimited", "ServerError", "NetworkError", "Timeout", "AuthenticationError", "AuthorizationError"],
        },
        ErrorTypeMetadata {
            name: "ToolError",
            file: "tools/spec.rs",
            description: "工具执行错误",
            variants: &["ExecutionFailed", "InvalidInput", "Timeout", "NotFound"],
        },
        ErrorTypeMetadata {
            name: "ArgRepairError",
            file: "tools/arg_repair.rs",
            description: "参数修复错误",
            variants: &["RepairFailed", "InvalidSchema"],
        },
        ErrorTypeMetadata {
            name: "FimError",
            file: "tools/fim.rs",
            description: "FIM 错误",
            variants: &["RequestFailed", "InvalidResponse"],
        },
        ErrorTypeMetadata {
            name: "ProjectContextError",
            file: "project_context.rs",
            description: "项目上下文错误",
            variants: &["LoadFailed", "ParseError"],
        },
        ErrorTypeMetadata {
            name: "ApiKeyError",
            file: "tui/app.rs",
            description: "API Key 错误",
            variants: &["NotFound", "Invalid", "Expired"],
        },
    ]
}

/// 按名称查找错误类型
pub fn find_error_type(name: &str) -> Option<ErrorTypeMetadata> {
    all_error_type_metadata().into_iter().find(|e| e.name == name)
}

/// 按文件查找错误类型
pub fn error_types_in_file(file: &str) -> Vec<ErrorTypeMetadata> {
    all_error_type_metadata()
        .into_iter()
        .filter(|e| e.file == file)
        .collect()
}
