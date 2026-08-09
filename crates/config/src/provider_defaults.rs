//! Built-in provider default seeds: per-mode default model ids and
//! base URLs, plus the generic model constants used across the app.
//! Extracted from `lib.rs` (#3311). Re-exported `pub` at the crate root so
//! existing `crate::DEFAULT_*` references (and cross-crate
//! `mimofan_config::*` paths) keep resolving.

use crate::ProviderKind;

// ── Default provider ────────────────────────────────────────────────
/// Default provider ID used when no provider is specified.
pub const DEFAULT_PROVIDER_ID: &str = ProviderKind::OpenAiCompatible.as_str();

// ── Environment variable names (generic, mode-agnostic) ─────────────
/// Standard API key environment variables for the OpenAI-compatible mode.
pub const OPENAI_COMPATIBLE_ENV_VARS: &[&str] = &["OPENAI_API_KEY", "MIMOFAN_API_KEY"];

/// Anthropic-compatible mode API key environment variables.
pub const ANTHROPIC_COMPATIBLE_ENV_VARS: &[&str] =
    &["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN", "MIMOFAN_API_KEY"];

/// Gemini-compatible mode API key environment variables.
pub const GEMINI_COMPATIBLE_ENV_VARS: &[&str] =
    &["GEMINI_API_KEY", "GOOGLE_API_KEY", "MIMOFAN_API_KEY"];

// ── Default models per mode ────────────────────────────────────────
pub const DEFAULT_OPENAI_COMPATIBLE_MODEL: &str = "gpt-4o";
pub const DEFAULT_ANTHROPIC_COMPATIBLE_MODEL: &str = "claude-sonnet-4-0";
pub const DEFAULT_GEMINI_COMPATIBLE_MODEL: &str = "gemini-2.0-flash";

// ── Default base URLs per mode (placeholders / public defaults) ────
/// Loopback placeholder: a misconfigured OpenAI-compatible provider fails
/// closed locally rather than reaching a public host.
pub const DEFAULT_OPENAI_COMPATIBLE_BASE_URL: &str = "http://localhost/v1";
pub const DEFAULT_ANTHROPIC_COMPATIBLE_BASE_URL: &str = "https://api.anthropic.com/v1";
pub const DEFAULT_GEMINI_COMPATIBLE_BASE_URL: &str =
    "https://generativelanguage.googleapis.com/v1beta";
