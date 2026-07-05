//! Static provider model-name and base-URL constants.
//!
//! These are pure data tables (default model identifiers, base URLs, and
//! curated model lists) extracted verbatim from `config.rs` to keep the
//! configuration monolith focused on loading/normalization logic. They are
//! re-exported from `crate::config` via `pub use models::*;`, so every existing
//! `crate::config::<CONST>` path keeps resolving unchanged (#3311).

// Default text model is `mimo-v2.5-pro` because the default API provider
// (`XiaomiMimo`, see `Config::api_provider()`) speaks the Xiaomi MiMo dialect
// and exposes this as its flagship model. The `Config::default_model()` method
// resolves this in conjunction with any per-provider `default_text_model` in
// `config.toml`, so per-provider overrides still win.
pub const DEFAULT_TEXT_MODEL: &str = "mimo-v2.5-pro";
// Default MiMo base URL points at the Xiaomi MiMo Anthropic-compatible
// gateway. The `base_url` field is the primary signal mimofan uses to
// decide which wire dialect to speak: when it ends in `/anthropic`
// (`api_provider_uses_anthropic_messages`), we use the native Anthropic
// Messages API; otherwise we fall back to the OpenAI-compatible
// `/v1/chat/completions` dialect. See `client::anthropic_messages_url`
// and `client::api_provider_uses_anthropic_messages` for the routing.
pub const DEFAULT_MIMO_BASE_URL: &str = "https://api.xiaomimimo.com/anthropic";
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
