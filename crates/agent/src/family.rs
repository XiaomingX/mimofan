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
