//! The canonical [`ProviderKind`] enum: the set of supported provider kinds.
//!
//! mimofan 仅支持 XiaomiMiMo 作为内置 provider，以及 Custom 用于用户自定义
//! OpenAI-compatible endpoint。

use serde::{Deserialize, Serialize};

use crate::provider;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderKind {
    /// Xiaomi MiMo — 唯一内置 provider
    #[default]
    #[serde(alias = "mimo", alias = "xiaomi", alias = "xiaomi_mimo")]
    XiaomiMimo,
    /// 用户自定义 OpenAI-compatible endpoint
    ///
    /// 用于 `[providers.<name>] kind="openai-compatible"` 配置。
    /// 使用 OpenAI Chat Completions 协议，base_url 和 model 通过配置指定。
    Custom,
}

impl ProviderKind {
    pub const ALL: [Self; 2] = [Self::XiaomiMimo, Self::Custom];

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
    pub fn as_str(self) -> &'static str {
        self.provider().id()
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

    #[must_use]
    pub fn is_siliconflow(self) -> bool {
        false
    }

    /// Return the built-in metadata entry for this provider.
    #[must_use]
    pub fn provider(self) -> &'static dyn provider::Provider {
        provider::provider_for_kind(self)
    }
}
