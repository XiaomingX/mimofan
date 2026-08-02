//! Static provider model-name and base-URL constants.
//!
//! The Xiaomi MiMo model ids and base URLs are the single source of truth in
//! `mimofan_config::provider_defaults` (consolidated in #3311). They are
//! re-exported here so every existing `crate::config::<CONST>` path keeps
//! resolving unchanged. TUI-only constants (the
//! DeepSeek model list, the Z.ai GLM model ids, and the Anthropic-compatible
//! MiMo gateway URL) remain defined here.

// The default text model for this app is the default Xiaomi MiMo model.
pub use mimofan_config::DEFAULT_XIAOMI_MIMO_MODEL;
pub use mimofan_config::DEFAULT_XIAOMI_MIMO_MODEL as DEFAULT_TEXT_MODEL;

// Anthropic-compatible MiMo gateway. This is a distinct endpoint from the
// token-plan base URL (`DEFAULT_XIAOMI_MIMO_BASE_URL`) and intentionally kept
// separate: its `/anthropic` suffix selects the native Anthropic Messages
// dialect instead of the OpenAI-compatible `/v1/chat/completions` dialect.
pub const DEFAULT_MIMO_BASE_URL: &str = "https://api.xiaomimimo.com/anthropic";

pub use mimofan_config::DEFAULT_XIAOMI_MIMO_BASE_URL;
pub use mimofan_config::XIAOMI_MIMO_ANTHROPIC_BASE_URL;
pub use mimofan_config::XIAOMI_MIMO_TOKEN_PLAN_CN_BASE_URL;
pub use mimofan_config::XIAOMI_MIMO_V2_5_PRO_ULTRASPEED_MODEL;

pub const COMMON_MIMOFAN_MODELS: &[&str] = &[
    "deepseek-v4-pro",
    "deepseek-v4-flash",
    "deepseek-ai/deepseek-v4-pro",
    "deepseek-ai/deepseek-v4-flash",
    "deepseek/deepseek-v4-pro",
    "deepseek/deepseek-v4-flash",
];
pub const ZAI_GLM_5_2_MODEL: &str = "GLM-5.2";
pub const ZAI_GLM_5_TURBO_MODEL: &str = "GLM-5-Turbo";
