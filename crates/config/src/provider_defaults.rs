//! Built-in provider default seeds: per-provider default model ids and
//! base URLs, plus the named model/tier constants the alias-normalization
//! tables resolve to. Extracted verbatim from `lib.rs` (#3311) to separate
//! these provider execution defaults from config schema/loading code; values
//! are unchanged. Re-exported `pub` at the crate root so existing
//! `crate::DEFAULT_*` references (and cross-crate `mimofan_config::*` paths)
//! keep resolving.

use crate::ProviderKind;

// ── Default provider ────────────────────────────────────────────────
/// Default provider ID used when no provider is specified.
pub const DEFAULT_PROVIDER_ID: &str = ProviderKind::XiaomiMimo.as_str();

// ── Environment variable names ──────────────────────────────────────
/// Standard API key environment variables for Xiaomi MiMo (pay-as-you-go).
pub const XIAOMI_MIMO_STANDARD_ENV_VARS: &[&str] = &["XIAOMI_MIMO_API_KEY", "ANTHROPIC_API_KEY"];

/// Token plan API key environment variables for Xiaomi MiMo.
pub const XIAOMI_MIMO_TOKEN_PLAN_ENV_VARS: &[&str] = &["XIAOMI_MIMO_TOKEN_PLAN_API_KEY"];

// ── DeepSeek defaults ────────────────────────────────────────────────
pub const DEFAULT_MIMOFAN_MODEL: &str = "deepseek-v4-pro";
pub const DEFAULT_MIMOFAN_FLASH_MODEL: &str = "deepseek-v4-flash";

// ── Xiaomi MiMo defaults ────────────────────────────────────────────
pub const DEFAULT_XIAOMI_MIMO_MODEL: &str = "mimo-v2.5-pro";
pub const XIAOMI_MIMO_V2_5_PRO_ULTRASPEED_MODEL: &str = "mimo-v2.5-pro-ultraspeed";
pub const XIAOMI_MIMO_ASR_MODEL: &str = "mimo-v2.5-asr";
pub const XIAOMI_MIMO_TTS_MODEL: &str = "mimo-v2.5-tts";
pub const XIAOMI_MIMO_TTS_VOICE_DESIGN_MODEL: &str = "mimo-v2.5-tts-voicedesign";
pub const XIAOMI_MIMO_TTS_VOICE_CLONE_MODEL: &str = "mimo-v2.5-tts-voiceclone";
pub const XIAOMI_MIMO_V2_TTS_MODEL: &str = "mimo-v2-tts";
pub const XIAOMI_MIMO_PAY_AS_YOU_GO_BASE_URL: &str = "https://api.xiaomimimo.com/v1";
pub const XIAOMI_MIMO_ANTHROPIC_BASE_URL: &str = "https://api.xiaomimimo.com/anthropic";
pub const DEFAULT_XIAOMI_MIMO_BASE_URL: &str = "https://token-plan-sgp.xiaomimimo.com/v1";
pub const XIAOMI_MIMO_TOKEN_PLAN_CN_BASE_URL: &str = "https://token-plan-cn.xiaomimimo.com/v1";
