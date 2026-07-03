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
