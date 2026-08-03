use mimofan_agent::*;

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
