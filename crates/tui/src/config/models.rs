//! Static provider model-name and base-URL constants.
//!
//! 单一来源是 `mimofan_config::provider_defaults`。此处仅保留 TUI 内部使用的
//! 通用默认常量（不再绑定任何具体产品）。

// The default text model for this app is the generic OpenAI-compatible default.
pub use mimofan_config::DEFAULT_OPENAI_COMPATIBLE_MODEL;
pub use mimofan_config::DEFAULT_OPENAI_COMPATIBLE_MODEL as DEFAULT_TEXT_MODEL;

// 常用模型列表，供帮助文案与补全参考（产品无关）。
pub const COMMON_MODELS: &[&str] = &[
    "gpt-4o",
    "gpt-4o-mini",
    "claude-sonnet-4-0",
    "claude-opus-4-0",
    "gemini-2.0-flash",
    "gemini-2.5-pro",
    "deepseek-v4-pro",
    "deepseek-v4-flash",
    "deepseek-ai/deepseek-v4-pro",
    "deepseek-ai/deepseek-v4-flash",
    "deepseek/deepseek-v4-pro",
    "deepseek/deepseek-v4-flash",
];

pub const ZAI_GLM_5_2_MODEL: &str = "GLM-5.2";
pub const ZAI_GLM_5_TURBO_MODEL: &str = "GLM-5-Turbo";
