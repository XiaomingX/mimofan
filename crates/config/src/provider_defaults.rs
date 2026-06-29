//! Built-in provider default seeds: per-provider default model ids and
//! base URLs, plus the named model/tier constants the alias-normalization
//! tables resolve to. Extracted verbatim from `lib.rs` (#3311) to separate
//! these provider execution defaults from config schema/loading code; values
//! are unchanged. Re-exported `pub(crate)` at the crate root so existing
//! `crate::DEFAULT_*` references keep resolving.

// ── Default provider ────────────────────────────────────────────────
/// Default provider ID used when no provider is specified.
pub const DEFAULT_PROVIDER_ID: &str = "xiaomi-mimo";

// ── Xiaomi MiMo defaults ────────────────────────────────────────────
pub(crate) const DEFAULT_XIAOMI_MIMO_MODEL: &str = "mimo-v2.5-pro";
pub(crate) const XIAOMI_MIMO_V2_5_PRO_ULTRASPEED_MODEL: &str = "mimo-v2.5-pro-ultraspeed";
pub(crate) const XIAOMI_MIMO_V2_5_OMNI_MODEL: &str = "mimo-v2.5";
pub(crate) const XIAOMI_MIMO_ASR_MODEL: &str = "mimo-v2.5-asr";
pub(crate) const XIAOMI_MIMO_TTS_MODEL: &str = "mimo-v2.5-tts";
pub(crate) const XIAOMI_MIMO_TTS_VOICE_DESIGN_MODEL: &str = "mimo-v2.5-tts-voicedesign";
pub(crate) const XIAOMI_MIMO_TTS_VOICE_CLONE_MODEL: &str = "mimo-v2.5-tts-voiceclone";
pub(crate) const XIAOMI_MIMO_V2_TTS_MODEL: &str = "mimo-v2-tts";
pub(crate) const XIAOMI_MIMO_PAY_AS_YOU_GO_BASE_URL: &str = "https://api.xiaomimimo.com/v1";
pub(crate) const DEFAULT_XIAOMI_MIMO_BASE_URL: &str = "https://token-plan-sgp.xiaomimimo.com/v1";
pub(crate) const XIAOMI_MIMO_TOKEN_PLAN_CN_BASE_URL: &str =
    "https://token-plan-cn.xiaomimimo.com/v1";
pub(crate) const XIAOMI_MIMO_TOKEN_PLAN_SGP_BASE_URL: &str = DEFAULT_XIAOMI_MIMO_BASE_URL;
pub(crate) const XIAOMI_MIMO_TOKEN_PLAN_AMS_BASE_URL: &str =
    "https://token-plan-ams.xiaomimimo.com/v1";
