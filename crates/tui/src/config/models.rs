//! Static provider model-name and base-URL constants.
//!
//! These are pure data tables (default model identifiers, base URLs, and
//! curated model lists) extracted verbatim from `config.rs` to keep the
//! configuration monolith focused on loading/normalization logic. They are
//! re-exported from `crate::config` via `pub use models::*;`, so every existing
//! `crate::config::<CONST>` path keeps resolving unchanged (#3311).

pub const DEFAULT_TEXT_MODEL: &str = "deepseek-v4-pro";
pub const DEFAULT_MIMO_BASE_URL: &str = "https://api.deepseek.com/beta";
pub const DEFAULT_NVIDIA_NIM_BASE_URL: &str = "https://integrate.api.nvidia.com/v1";
pub const DEFAULT_XIAOMI_MIMO_MODEL: &str = "mimo-v2.5-pro";
pub const XIAOMI_MIMO_V2_5_PRO_ULTRASPEED_MODEL: &str = "mimo-v2.5-pro-ultraspeed";
pub const XIAOMI_MIMO_PAY_AS_YOU_GO_BASE_URL: &str = "https://api.xiaomimimo.com/v1";
pub const DEFAULT_XIAOMI_MIMO_BASE_URL: &str = "https://token-plan-sgp.xiaomimimo.com/v1";
pub const XIAOMI_MIMO_TOKEN_PLAN_CN_BASE_URL: &str = "https://token-plan-cn.xiaomimimo.com/v1";
pub const XIAOMI_MIMO_TOKEN_PLAN_SGP_BASE_URL: &str = DEFAULT_XIAOMI_MIMO_BASE_URL;
pub const XIAOMI_MIMO_TOKEN_PLAN_AMS_BASE_URL: &str = "https://token-plan-ams.xiaomimimo.com/v1";
pub const XIAOMI_MIMO_V2_5_OMNI_MODEL: &str = "mimo-v2.5";
pub const XIAOMI_MIMO_ASR_MODEL: &str = "mimo-v2.5-asr";
pub const XIAOMI_MIMO_TTS_MODEL: &str = "mimo-v2.5-tts";
pub const XIAOMI_MIMO_TTS_VOICE_DESIGN_MODEL: &str = "mimo-v2.5-tts-voicedesign";
pub const XIAOMI_MIMO_TTS_VOICE_CLONE_MODEL: &str = "mimo-v2.5-tts-voiceclone";
pub const XIAOMI_MIMO_V2_TTS_MODEL: &str = "mimo-v2-tts";
pub const COMMON_DEEPSEEK_MODELS: &[&str] = &[
    "deepseek-v4-pro",
    "deepseek-v4-flash",
    "deepseek-ai/deepseek-v4-pro",
    "deepseek-ai/deepseek-v4-flash",
    "deepseek/deepseek-v4-pro",
    "deepseek/deepseek-v4-flash",
];
pub const ZAI_GLM_5_2_MODEL: &str = "GLM-5.2";
pub const ZAI_GLM_5_TURBO_MODEL: &str = "GLM-5-Turbo";
