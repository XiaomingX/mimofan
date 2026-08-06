//! The canonical [`ProviderKind`] enum: the set of supported wire-protocol
//! compat modes.
//!
//! mimofan 不再绑定任何具体产品（如 Xiaomi MiMo）。它只关心 LLM 网关
//! 所「说」的线协议（wire protocol）。任何端点只要兼容下列三种协议之一，
//! 即可被 mimofan 直接对接——base_url、model、api_key 全部来自配置。
//!
//! 三种模式彼此互斥且完备（MECE）：
//! - `OpenAiCompatible`    —— OpenAI `/v1/chat/completions` 风格端点
//! - `AnthropicCompatible` —— Anthropic `/v1/messages` 原生协议端点
//! - `GeminiCompatible`    —— Google Gemini `generativelanguage` 协议端点

use serde::{Deserialize, Serialize};

use crate::provider;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderKind {
    /// OpenAI-compatible `/v1/chat/completions` endpoint.
    ///
    /// 涵盖所有兼容 OpenAI Chat Completions 协议的自建/第三方网关
    ///（原 Xiaomi MiMo、原 Custom 等均归于此，统一为一种模式）。
    #[default]
    #[serde(alias = "openai", alias = "openai-compatible", alias = "custom", alias = "xiaomi-mimo", alias = "mimo")]
    OpenAiCompatible,
    /// Anthropic Messages API compatible endpoint (`/v1/messages`).
    #[serde(alias = "anthropic", alias = "anthropic-compatible")]
    AnthropicCompatible,
    /// Google Gemini compatible endpoint
    /// (`generativelanguage.googleapis.com/v1beta/models/...:generateContent`).
    #[serde(alias = "gemini", alias = "gemini-compatible", alias = "google")]
    GeminiCompatible,
}

impl ProviderKind {
    pub const ALL: [Self; 3] = [
        Self::OpenAiCompatible,
        Self::AnthropicCompatible,
        Self::GeminiCompatible,
    ];

    #[must_use]
    pub fn all() -> &'static [Self] {
        &Self::ALL
    }

    #[must_use]
    pub fn names_hint() -> String {
        Self::all()
            .iter()
            .map(|provider| provider.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            ProviderKind::OpenAiCompatible => "openai-compatible",
            ProviderKind::AnthropicCompatible => "anthropic-compatible",
            ProviderKind::GeminiCompatible => "gemini-compatible",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        let trimmed = value.trim();
        provider::all_providers()
            .iter()
            .find(|p| {
                trimmed.eq_ignore_ascii_case(p.id())
                    || p.aliases().iter().any(|a| trimmed.eq_ignore_ascii_case(a))
            })
            .map(|p| p.kind())
    }

    /// Return the built-in metadata entry for this compat mode.
    #[must_use]
    pub fn provider(self) -> &'static dyn provider::Provider {
        provider::provider_for_kind(self)
    }

    /// 当前模式对应的线协议（wire protocol）。
    #[must_use]
    pub fn wire(self) -> crate::provider::WireFormat {
        self.provider().wire()
    }
}
