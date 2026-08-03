use std::collections::HashMap;

use mimofan_config::ProviderKind;
use serde::{Deserialize, Serialize};

pub mod family;
pub mod provider_resolver;

use provider_resolver::{
    arcee_passthrough_model, model_matches, normalize, preserve_requested_model_id_case,
    xiaomi_mimo_passthrough_model,
};

// Re-export for backward compatibility
pub use family::{ModelFamily, model_family};

/// Metadata for a single model entry in the registry.
///
/// Each model has a canonical `id` used by the provider, a list of `aliases`
/// that users may reference, and capability flags indicating whether the model
/// supports tool use and reasoning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    /// The canonical model identifier used by the provider (e.g. `"deepseek-v4-pro"`).
    pub id: String,
    /// The provider that serves this model.
    pub provider: ProviderKind,
    /// Alternative names that users can use to reference this model (case-insensitive).
    pub aliases: Vec<String>,
    /// Whether this model supports tool/function calling.
    pub supports_tools: bool,
    /// Whether this model supports extended reasoning.
    pub supports_reasoning: bool,
}

/// The result of resolving a user-requested model name to a concrete model entry.
///
/// Contains the resolved [`ModelInfo`], whether a fallback was used, and the
/// chain of resolution strategies that were attempted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelResolution {
    /// The original model name requested by the user, if any.
    pub requested: Option<String>,
    /// The concrete model that was resolved.
    pub resolved: ModelInfo,
    /// Whether a fallback was used because the requested model was not found.
    pub used_fallback: bool,
    /// The ordered list of resolution strategies that were attempted.
    pub fallback_chain: Vec<String>,
}

/// A registry of supported models and their aliases, used to resolve user-facing
/// model names to concrete provider-specific model entries.
///
/// The default registry is populated with all built-in models across supported
/// providers (DeepSeek, NVIDIA NIM, OpenAI-compatible, and others).
#[derive(Debug, Clone)]
pub struct ModelRegistry {
    models: Vec<ModelInfo>,
    alias_map: HashMap<String, usize>,
}

/// Creates a registry pre-populated with all built-in models and their aliases.
impl Default for ModelRegistry {
    fn default() -> Self {
        let models = vec![
            ModelInfo {
                id: "deepseek-v4-pro".to_string(),
                provider: ProviderKind::XiaomiMimo,
                aliases: vec![],
                supports_tools: true,
                supports_reasoning: true,
            },
            ModelInfo {
                id: "deepseek-v4-flash".to_string(),
                provider: ProviderKind::XiaomiMimo,
                aliases: vec!["deepseek-chat".to_string()],
                supports_tools: true,
                supports_reasoning: true,
            },
            ModelInfo {
                id: "deepseek-ai/deepseek-v4-pro".to_string(),
                provider: ProviderKind::XiaomiMimo,
                aliases: vec![
                    "deepseek-v4-pro".to_string(),
                    "nvidia-deepseek-v4-pro".to_string(),
                    "nim-deepseek-v4-pro".to_string(),
                ],
                supports_tools: true,
                supports_reasoning: true,
            },
            ModelInfo {
                id: "deepseek-ai/deepseek-v4-flash".to_string(),
                provider: ProviderKind::XiaomiMimo,
                aliases: vec![
                    "deepseek-v4-flash".to_string(),
                    "deepseek-chat".to_string(),
                    "deepseek-reasoner".to_string(),
                    "nvidia-deepseek-v4-flash".to_string(),
                    "nim-deepseek-v4-flash".to_string(),
                ],
                supports_tools: true,
                supports_reasoning: true,
            },
            ModelInfo {
                id: "deepseek-v4-pro".to_string(),
                provider: ProviderKind::XiaomiMimo,
                aliases: vec!["openai-compatible-deepseek-v4-pro".to_string()],
                supports_tools: true,
                supports_reasoning: true,
            },
            ModelInfo {
                id: "deepseek-v4-flash".to_string(),
                provider: ProviderKind::XiaomiMimo,
                aliases: vec!["openai-compatible-deepseek-v4-flash".to_string()],
                supports_tools: true,
                supports_reasoning: true,
            },
            ModelInfo {
                id: "deepseek-ai/deepseek-v4-flash".to_string(),
                provider: ProviderKind::XiaomiMimo,
                aliases: vec!["deepseek-v4-flash".to_string()],
                supports_tools: true,
                supports_reasoning: true,
            },
            ModelInfo {
                id: "deepseek-ai/deepseek-v4-pro".to_string(),
                provider: ProviderKind::XiaomiMimo,
                aliases: vec!["deepseek-v4-pro".to_string()],
                supports_tools: true,
                supports_reasoning: true,
            },
            ModelInfo {
                id: "DeepSeek-V4-Pro".to_string(),
                provider: ProviderKind::XiaomiMimo,
                aliases: vec![
                    "deepseek-v4-pro".to_string(),
                    "volcengine-deepseek-v4-pro".to_string(),
                    "ark-deepseek-v4-pro".to_string(),
                ],
                supports_tools: true,
                supports_reasoning: true,
            },
            ModelInfo {
                id: "DeepSeek-V4-Flash".to_string(),
                provider: ProviderKind::XiaomiMimo,
                aliases: vec![
                    "deepseek-v4-flash".to_string(),
                    "deepseek-chat".to_string(),
                    "volcengine-deepseek-v4-flash".to_string(),
                    "ark-deepseek-v4-flash".to_string(),
                ],
                supports_tools: true,
                supports_reasoning: true,
            },
            ModelInfo {
                id: "deepseek/deepseek-v4-pro".to_string(),
                provider: ProviderKind::XiaomiMimo,
                aliases: vec![
                    "deepseek-v4-pro".to_string(),
                    "openrouter-deepseek-v4-pro".to_string(),
                ],
                supports_tools: true,
                supports_reasoning: true,
            },
            ModelInfo {
                id: "deepseek/deepseek-v4-flash".to_string(),
                provider: ProviderKind::XiaomiMimo,
                aliases: vec![
                    "deepseek-v4-flash".to_string(),
                    "deepseek-chat".to_string(),
                    "deepseek-reasoner".to_string(),
                    "openrouter-deepseek-v4-flash".to_string(),
                ],
                supports_tools: true,
                supports_reasoning: true,
            },
            ModelInfo {
                id: "arcee-ai/trinity-large-thinking".to_string(),
                provider: ProviderKind::XiaomiMimo,
                aliases: vec![
                    "trinity".to_string(),
                    "trinity-large-thinking".to_string(),
                    "arcee-trinity-large-thinking".to_string(),
                ],
                supports_tools: true,
                supports_reasoning: true,
            },
            ModelInfo {
                id: "xiaomi/mimo-v2.5-pro".to_string(),
                provider: ProviderKind::XiaomiMimo,
                aliases: vec![
                    "openrouter-mimo-v2.5-pro".to_string(),
                    "openrouter-xiaomi-mimo-v2.5-pro".to_string(),
                ],
                supports_tools: true,
                supports_reasoning: true,
            },
            ModelInfo {
                id: "xiaomi/mimo-v2.5".to_string(),
                provider: ProviderKind::XiaomiMimo,
                aliases: vec![
                    "openrouter-mimo-v2.5".to_string(),
                    "openrouter-xiaomi-mimo-v2.5".to_string(),
                ],
                supports_tools: true,
                supports_reasoning: true,
            },
            ModelInfo {
                id: "qwen/qwen3.6-flash".to_string(),
                provider: ProviderKind::XiaomiMimo,
                aliases: vec!["qwen3.6-flash".to_string(), "qwen-3.6-flash".to_string()],
                supports_tools: true,
                supports_reasoning: true,
            },
            ModelInfo {
                id: "qwen/qwen3.6-35b-a3b".to_string(),
                provider: ProviderKind::XiaomiMimo,
                aliases: vec![
                    "qwen3.6-35b-a3b".to_string(),
                    "qwen-3.6-35b-a3b".to_string(),
                ],
                supports_tools: true,
                supports_reasoning: true,
            },
            ModelInfo {
                id: "qwen/qwen3.6-max-preview".to_string(),
                provider: ProviderKind::XiaomiMimo,
                aliases: vec![
                    "qwen3.6-max-preview".to_string(),
                    "qwen-3.6-max-preview".to_string(),
                    "qwen-max-preview".to_string(),
                ],
                supports_tools: true,
                supports_reasoning: true,
            },
            ModelInfo {
                id: "qwen/qwen3.6-27b".to_string(),
                provider: ProviderKind::XiaomiMimo,
                aliases: vec!["qwen3.6-27b".to_string(), "qwen-3.6-27b".to_string()],
                supports_tools: true,
                supports_reasoning: true,
            },
            ModelInfo {
                id: "qwen/qwen3.6-plus".to_string(),
                provider: ProviderKind::XiaomiMimo,
                aliases: vec!["qwen3.6-plus".to_string(), "qwen-3.6-plus".to_string()],
                supports_tools: true,
                supports_reasoning: true,
            },
            ModelInfo {
                id: "moonshotai/kimi-k2.7-code".to_string(),
                provider: ProviderKind::XiaomiMimo,
                aliases: vec![
                    "kimi-k2.7-code".to_string(),
                    "openrouter-kimi-k2.7-code".to_string(),
                ],
                supports_tools: true,
                supports_reasoning: true,
            },
            ModelInfo {
                id: "minimax/minimax-m3".to_string(),
                provider: ProviderKind::XiaomiMimo,
                aliases: vec!["openrouter-minimax-m3".to_string()],
                supports_tools: true,
                supports_reasoning: true,
            },
            ModelInfo {
                id: "z-ai/glm-5.2".to_string(),
                provider: ProviderKind::XiaomiMimo,
                aliases: vec!["glm-5.2".to_string(), "zai-glm-5.2".to_string()],
                supports_tools: true,
                supports_reasoning: true,
            },
            ModelInfo {
                id: "GLM-5.2".to_string(),
                provider: ProviderKind::XiaomiMimo,
                aliases: vec![
                    "glm-5.2".to_string(),
                    "glm-5-2".to_string(),
                    "zai-glm-5.2".to_string(),
                    "zai-glm-5-2".to_string(),
                ],
                supports_tools: true,
                supports_reasoning: true,
            },
            ModelInfo {
                id: "GLM-5-Turbo".to_string(),
                provider: ProviderKind::XiaomiMimo,
                aliases: vec![
                    "glm-5-turbo".to_string(),
                    "glm-5turbo".to_string(),
                    "zai-glm-5-turbo".to_string(),
                ],
                supports_tools: true,
                supports_reasoning: true,
            },
            ModelInfo {
                id: "tencent/hy3-preview".to_string(),
                provider: ProviderKind::XiaomiMimo,
                aliases: vec!["hy3-preview".to_string(), "tencent-hy3-preview".to_string()],
                supports_tools: true,
                supports_reasoning: true,
            },
            ModelInfo {
                id: "mimo-v2.5-pro".to_string(),
                provider: ProviderKind::XiaomiMimo,
                aliases: vec!["mimo".to_string(), "pro".to_string()],
                supports_tools: true,
                supports_reasoning: true,
            },
            ModelInfo {
                id: "mimo-v2.5".to_string(),
                provider: ProviderKind::XiaomiMimo,
                aliases: vec![
                    "omni".to_string(),
                    "mimo-omni".to_string(),
                    "v2.5-omni".to_string(),
                    "mimo-v2.5-omni".to_string(),
                ],
                supports_tools: true,
                supports_reasoning: true,
            },
            ModelInfo {
                id: "mimo-v2.5-asr".to_string(),
                provider: ProviderKind::XiaomiMimo,
                aliases: vec![
                    "asr".to_string(),
                    "speech-to-text".to_string(),
                    "transcribe".to_string(),
                ],
                supports_tools: false,
                supports_reasoning: false,
            },
            ModelInfo {
                id: "mimo-v2.5-tts".to_string(),
                provider: ProviderKind::XiaomiMimo,
                aliases: vec![
                    "tts".to_string(),
                    "speech".to_string(),
                    "mimo-tts".to_string(),
                ],
                supports_tools: false,
                supports_reasoning: false,
            },
            ModelInfo {
                id: "mimo-v2.5-tts-voicedesign".to_string(),
                provider: ProviderKind::XiaomiMimo,
                aliases: vec![
                    "voicedesign".to_string(),
                    "voice-design".to_string(),
                    "mimo-voice-design".to_string(),
                ],
                supports_tools: false,
                supports_reasoning: false,
            },
            ModelInfo {
                id: "mimo-v2.5-tts-voiceclone".to_string(),
                provider: ProviderKind::XiaomiMimo,
                aliases: vec![
                    "voiceclone".to_string(),
                    "voice-clone".to_string(),
                    "mimo-voice-clone".to_string(),
                ],
                supports_tools: false,
                supports_reasoning: false,
            },
            ModelInfo {
                id: "mimo-v2-tts".to_string(),
                provider: ProviderKind::XiaomiMimo,
                aliases: vec!["mimo-v2-speech".to_string()],
                supports_tools: false,
                supports_reasoning: false,
            },
            ModelInfo {
                id: "deepseek/deepseek-v4-pro".to_string(),
                provider: ProviderKind::XiaomiMimo,
                aliases: vec![
                    "deepseek-v4-pro".to_string(),
                    "novita-deepseek-v4-pro".to_string(),
                ],
                supports_tools: true,
                supports_reasoning: true,
            },
            ModelInfo {
                id: "deepseek/deepseek-v4-flash".to_string(),
                provider: ProviderKind::XiaomiMimo,
                aliases: vec![
                    "deepseek-v4-flash".to_string(),
                    "deepseek-chat".to_string(),
                    "deepseek-reasoner".to_string(),
                    "novita-deepseek-v4-flash".to_string(),
                ],
                supports_tools: true,
                supports_reasoning: true,
            },
            ModelInfo {
                id: "accounts/fireworks/models/deepseek-v4-pro".to_string(),
                provider: ProviderKind::XiaomiMimo,
                aliases: vec![
                    "deepseek-v4-pro".to_string(),
                    "fireworks-deepseek-v4-pro".to_string(),
                ],
                supports_tools: true,
                supports_reasoning: true,
            },
            ModelInfo {
                id: "deepseek-ai/DeepSeek-V4-Pro".to_string(),
                provider: ProviderKind::XiaomiMimo,
                aliases: vec![
                    "deepseek-v4-pro".to_string(),
                    "deepseek-reasoner".to_string(),
                    "siliconflow-deepseek-v4-pro".to_string(),
                ],
                supports_tools: true,
                supports_reasoning: true,
            },
            ModelInfo {
                id: "deepseek-ai/DeepSeek-V4-Flash".to_string(),
                provider: ProviderKind::XiaomiMimo,
                aliases: vec![
                    "deepseek-v4-flash".to_string(),
                    "deepseek-chat".to_string(),
                    "siliconflow-deepseek-v4-flash".to_string(),
                ],
                supports_tools: true,
                supports_reasoning: true,
            },
            ModelInfo {
                id: "kimi-k2.7-code".to_string(),
                provider: ProviderKind::XiaomiMimo,
                aliases: vec![
                    "kimi".to_string(),
                    "kimi-k2.7".to_string(),
                    "kimi-code".to_string(),
                    "moonshot-kimi-k2.7-code".to_string(),
                ],
                supports_tools: true,
                supports_reasoning: true,
            },
            ModelInfo {
                id: "kimi-k2.6".to_string(),
                provider: ProviderKind::XiaomiMimo,
                aliases: vec!["kimi-k2.6".to_string(), "moonshot-kimi-k2.6".to_string()],
                supports_tools: true,
                supports_reasoning: true,
            },
            ModelInfo {
                id: "deepseek-ai/DeepSeek-V4-Pro".to_string(),
                provider: ProviderKind::XiaomiMimo,
                aliases: vec![
                    "deepseek-v4-pro".to_string(),
                    "hf-deepseek-v4-pro".to_string(),
                ],
                supports_tools: true,
                supports_reasoning: true,
            },
            ModelInfo {
                id: "deepseek-ai/DeepSeek-V4-Flash".to_string(),
                provider: ProviderKind::XiaomiMimo,
                aliases: vec![
                    "deepseek-v4-flash".to_string(),
                    "deepseek-chat".to_string(),
                    "deepseek-reasoner".to_string(),
                    "hf-deepseek-v4-flash".to_string(),
                ],
                supports_tools: true,
                supports_reasoning: true,
            },
            // Together AI provider models
            ModelInfo {
                id: "deepseek-ai/DeepSeek-V4-Pro".to_string(),
                provider: ProviderKind::XiaomiMimo,
                aliases: vec![
                    "deepseek-v4-pro".to_string(),
                    "together-deepseek-v4-pro".to_string(),
                ],
                supports_tools: true,
                supports_reasoning: true,
            },
            ModelInfo {
                id: "deepseek-ai/DeepSeek-V4-Flash".to_string(),
                provider: ProviderKind::XiaomiMimo,
                aliases: vec![
                    "deepseek-v4-flash".to_string(),
                    "deepseek-chat".to_string(),
                    "together-deepseek-v4-flash".to_string(),
                ],
                supports_tools: true,
                supports_reasoning: true,
            },
            // Qwen 3.7 Max (OpenRouter)
            ModelInfo {
                id: "qwen/qwen3.7-max".to_string(),
                provider: ProviderKind::XiaomiMimo,
                aliases: vec!["qwen3.7-max".to_string(), "qwen-3.7-max".to_string()],
                supports_tools: true,
                supports_reasoning: true,
            },
            // OpenAI Codex (ChatGPT OAuth) models
            ModelInfo {
                id: "gpt-5.5".to_string(),
                provider: ProviderKind::XiaomiMimo,
                aliases: vec!["codex-gpt-5.5".to_string(), "chatgpt-gpt-5.5".to_string()],
                supports_tools: true,
                supports_reasoning: true,
            },
            // Anthropic native Messages API models (#3014)
            ModelInfo {
                id: "claude-opus-4-8".to_string(),
                provider: ProviderKind::XiaomiMimo,
                aliases: vec!["opus".to_string(), "claude-opus".to_string()],
                supports_tools: true,
                supports_reasoning: true,
            },
            ModelInfo {
                id: "claude-sonnet-4-6".to_string(),
                provider: ProviderKind::XiaomiMimo,
                aliases: vec!["sonnet".to_string(), "claude-sonnet".to_string()],
                supports_tools: true,
                supports_reasoning: true,
            },
            ModelInfo {
                id: "claude-haiku-4-5".to_string(),
                provider: ProviderKind::XiaomiMimo,
                aliases: vec!["haiku".to_string(), "claude-haiku".to_string()],
                supports_tools: true,
                supports_reasoning: false,
            },
            // MiniMax 2.7 (OpenRouter)
            ModelInfo {
                id: "minimax/minimax-m2.7".to_string(),
                provider: ProviderKind::XiaomiMimo,
                aliases: vec![
                    "minimax-2.7".to_string(),
                    "minimax-2-7".to_string(),
                    "openrouter-minimax-2.7".to_string(),
                ],
                supports_tools: true,
                supports_reasoning: true,
            },
            ModelInfo {
                id: "MiniMax-M3".to_string(),
                provider: ProviderKind::XiaomiMimo,
                aliases: vec![
                    "minimax".to_string(),
                    "minimax-m3".to_string(),
                    "minimax-m-3".to_string(),
                ],
                supports_tools: true,
                supports_reasoning: true,
            },
            ModelInfo {
                id: "MiniMax-M2.7".to_string(),
                provider: ProviderKind::XiaomiMimo,
                aliases: vec![
                    "minimax-m2.7".to_string(),
                    "minimax-m2-7".to_string(),
                    "minimax-m-2.7".to_string(),
                    "minimax-m-2-7".to_string(),
                ],
                supports_tools: true,
                supports_reasoning: true,
            },
            ModelInfo {
                id: "MiniMax-M2.7-highspeed".to_string(),
                provider: ProviderKind::XiaomiMimo,
                aliases: vec![
                    "minimax-m2.7-highspeed".to_string(),
                    "minimax-m2-7-highspeed".to_string(),
                    "minimax-m-2.7-highspeed".to_string(),
                    "minimax-m-2-7-highspeed".to_string(),
                ],
                supports_tools: true,
                supports_reasoning: true,
            },
            ModelInfo {
                id: "MiniMax-M2.5".to_string(),
                provider: ProviderKind::XiaomiMimo,
                aliases: vec![
                    "minimax-m2.5".to_string(),
                    "minimax-m2-5".to_string(),
                    "minimax-m-2.5".to_string(),
                    "minimax-m-2-5".to_string(),
                ],
                supports_tools: true,
                supports_reasoning: true,
            },
            ModelInfo {
                id: "MiniMax-M2.5-highspeed".to_string(),
                provider: ProviderKind::XiaomiMimo,
                aliases: vec![
                    "minimax-m2.5-highspeed".to_string(),
                    "minimax-m2-5-highspeed".to_string(),
                    "minimax-m-2.5-highspeed".to_string(),
                    "minimax-m-2-5-highspeed".to_string(),
                ],
                supports_tools: true,
                supports_reasoning: true,
            },
            ModelInfo {
                id: "MiniMax-M2.1".to_string(),
                provider: ProviderKind::XiaomiMimo,
                aliases: vec![
                    "minimax-m2.1".to_string(),
                    "minimax-m2-1".to_string(),
                    "minimax-m-2.1".to_string(),
                    "minimax-m-2-1".to_string(),
                ],
                supports_tools: true,
                supports_reasoning: true,
            },
            ModelInfo {
                id: "MiniMax-M2.1-highspeed".to_string(),
                provider: ProviderKind::XiaomiMimo,
                aliases: vec![
                    "minimax-m2.1-highspeed".to_string(),
                    "minimax-m2-1-highspeed".to_string(),
                    "minimax-m-2.1-highspeed".to_string(),
                    "minimax-m-2-1-highspeed".to_string(),
                ],
                supports_tools: true,
                supports_reasoning: true,
            },
            ModelInfo {
                id: "MiniMax-M2".to_string(),
                provider: ProviderKind::XiaomiMimo,
                aliases: vec!["minimax-m2".to_string(), "minimax-m-2".to_string()],
                supports_tools: true,
                supports_reasoning: true,
            },
            // NVIDIA Nemotron 3 Ultra (OpenRouter)
            ModelInfo {
                id: "nvidia/nemotron-3-ultra-550b-a55b".to_string(),
                provider: ProviderKind::XiaomiMimo,
                aliases: vec![
                    "nvidia/nemotron-3-ultra".to_string(),
                    "nemotron-3-ultra".to_string(),
                    "nemotron-3-ultra-550b-a55b".to_string(),
                    "nvidia-nemotron-3-ultra".to_string(),
                    "nvidia-nemotron-3-ultra-550b-a55b".to_string(),
                ],
                supports_tools: true,
                supports_reasoning: true,
            },
            // DeepInfra (https://deepinfra.com)
            ModelInfo {
                id: "deepseek-ai/DeepSeek-V4-Pro".to_string(),
                provider: ProviderKind::XiaomiMimo,
                aliases: vec!["deepseek-v4-pro".to_string()],
                supports_tools: true,
                supports_reasoning: true,
            },
            ModelInfo {
                id: "deepseek-ai/DeepSeek-V4-Flash".to_string(),
                provider: ProviderKind::XiaomiMimo,
                aliases: vec![
                    "deepseek-v4-flash".to_string(),
                    "di-deepseek-v4-flash".to_string(),
                ],
                supports_tools: true,
                supports_reasoning: true,
            },
        ];
        Self::new(models)
    }
}

impl ModelRegistry {
    /// Creates a new registry from a list of [`ModelInfo`] entries.
    ///
    /// Builds an internal alias map for fast lookup by model id or alias.
    /// If multiple models share the same id or alias, the first one registered
    /// takes priority.
    #[must_use]
    pub fn new(models: Vec<ModelInfo>) -> Self {
        let mut alias_map = HashMap::new();
        for (idx, model) in models.iter().enumerate() {
            alias_map.entry(normalize(&model.id)).or_insert(idx);
            for alias in &model.aliases {
                alias_map.entry(normalize(alias)).or_insert(idx);
            }
        }
        Self { models, alias_map }
    }

    /// Returns a clone of all models in the registry.
    #[must_use]
    pub fn list(&self) -> Vec<ModelInfo> {
        self.models.clone()
    }

    /// Resolves a user-requested model name to a concrete [`ModelInfo`].
    ///
    /// Resolution follows this priority order:
    /// 1. If a `provider_hint` is given, search for a model matching that
    ///    provider whose id or alias matches the request (case-insensitive).
    /// 2. Look up the alias map for a case-insensitive match.
    /// 3. Fall back to the first model belonging to the hinted provider
    ///    (or DeepSeek if no hint was given).
    /// 4. As a last resort, fall back to the first model in the registry.
    #[must_use]
    pub fn resolve(
        &self,
        requested: Option<&str>,
        provider_hint: Option<ProviderKind>,
    ) -> ModelResolution {
        let mut fallback_chain = Vec::new();

        if let Some(name) = requested {
            fallback_chain.push(format!("requested:{name}"));
            if let Some(provider) = provider_hint
                && let Some(model) = self
                    .models
                    .iter()
                    .find(|m| m.provider == provider && model_matches(m, name))
                    .cloned()
            {
                return ModelResolution {
                    requested: Some(name.to_string()),
                    resolved: model,
                    used_fallback: false,
                    fallback_chain,
                };
            }
            if provider_hint == Some(ProviderKind::XiaomiMimo) {
                if let Some(model) = arcee_passthrough_model(name) {
                    return ModelResolution {
                        requested: Some(name.to_string()),
                        resolved: model,
                        used_fallback: false,
                        fallback_chain,
                    };
                }
                if let Some(model) = xiaomi_mimo_passthrough_model(name) {
                    return ModelResolution {
                        requested: Some(name.to_string()),
                        resolved: model,
                        used_fallback: false,
                        fallback_chain,
                    };
                }
            }
            if let Some(idx) = self.alias_map.get(&normalize(name)) {
                return ModelResolution {
                    requested: Some(name.to_string()),
                    resolved: preserve_requested_model_id_case(self.models[*idx].clone(), name),
                    used_fallback: false,
                    fallback_chain,
                };
            }
        }

        let provider = provider_hint.unwrap_or(ProviderKind::XiaomiMimo);
        fallback_chain.push(format!("provider_default:{}", provider.as_str()));
        if let Some(model) = self.models.iter().find(|m| m.provider == provider).cloned() {
            return ModelResolution {
                requested: requested.map(ToOwned::to_owned),
                resolved: model,
                used_fallback: true,
                fallback_chain,
            };
        }

        let final_fallback = self.models.first().cloned().unwrap_or(ModelInfo {
            id: "deepseek-v4-pro".to_string(),
            provider: ProviderKind::XiaomiMimo,
            aliases: Vec::new(),
            supports_tools: true,
            supports_reasoning: true,
        });
        fallback_chain.push("global_default:deepseek-v4-pro".to_string());
        ModelResolution {
            requested: requested.map(ToOwned::to_owned),
            resolved: final_fallback,
            used_fallback: true,
            fallback_chain,
        }
    }
}
