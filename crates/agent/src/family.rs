use serde::{Deserialize, Serialize};

/// High-level model family used for shared identity affordances across clients.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModelFamily {
    DeepSeek,
    Anthropic,
    OpenAI,
    Google,
    Meta,
    Mistral,
    Qwen,
    Grok,
    Cohere,
    GptOss,
    Inferencer,
}

/// Classify a model identifier by its underlying model family.
#[must_use]
pub fn model_family(model_id: &str) -> ModelFamily {
    let normalized = model_id.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return ModelFamily::Inferencer;
    }

    if normalized.contains("deepseek") {
        return ModelFamily::DeepSeek;
    }
    if normalized.contains("claude") || normalized.contains("anthropic") {
        return ModelFamily::Anthropic;
    }
    if normalized.contains("gpt-oss") || normalized.contains("gpt_oss") {
        return ModelFamily::GptOss;
    }
    if normalized.starts_with("gpt-")
        || normalized.contains("/gpt-")
        || normalized.contains("openai/")
    {
        return ModelFamily::OpenAI;
    }
    if normalized.contains("gemini")
        || normalized.contains("gemma")
        || normalized.contains("google/")
    {
        return ModelFamily::Google;
    }
    if normalized.contains("llama") || normalized.contains("meta-") || normalized.contains("meta/")
    {
        return ModelFamily::Meta;
    }
    if normalized.contains("mistral")
        || normalized.contains("mixtral")
        || normalized.contains("codestral")
    {
        return ModelFamily::Mistral;
    }
    if normalized.contains("qwen") {
        return ModelFamily::Qwen;
    }
    if normalized.contains("grok") {
        return ModelFamily::Grok;
    }
    if normalized.contains("cohere") || normalized.contains("command-r") {
        return ModelFamily::Cohere;
    }

    ModelFamily::Inferencer
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_family_classifies_known_model_ids() {
        assert_eq!(model_family("deepseek-v4-pro"), ModelFamily::DeepSeek);
        assert_eq!(model_family("openai/gpt-5.4"), ModelFamily::OpenAI);
        assert_eq!(
            model_family("anthropic/claude-opus-4-7"),
            ModelFamily::Anthropic
        );
        assert_eq!(
            model_family("meta-llama/llama-3.3-70b-instruct"),
            ModelFamily::Meta
        );
        assert_eq!(model_family("Qwen/Qwen3-Coder"), ModelFamily::Qwen);
    }

    #[test]
    fn model_family_uses_underlying_model_for_router_ids() {
        assert_eq!(
            model_family("groq/llama-3.3-70b-versatile"),
            ModelFamily::Meta
        );
        assert_eq!(
            model_family("openrouter/openai/gpt-5.4"),
            ModelFamily::OpenAI
        );
        assert_eq!(
            model_family("fireworks/accounts/fireworks/models/deepseek-v4-pro"),
            ModelFamily::DeepSeek
        );
    }

    #[test]
    fn model_family_covers_prominent_google_and_mistral_model_names() {
        assert_eq!(model_family("google/gemma-3-27b-it"), ModelFamily::Google);
        assert_eq!(
            model_family("mistralai/mixtral-8x22b"),
            ModelFamily::Mistral
        );
        assert_eq!(model_family("codestral-latest"), ModelFamily::Mistral);
    }

    #[test]
    fn model_family_falls_back_to_inferencer_for_unknown_models() {
        assert_eq!(
            model_family("custom-gateway/my-private-model"),
            ModelFamily::Inferencer
        );
        assert_eq!(model_family(""), ModelFamily::Inferencer);
    }
}
