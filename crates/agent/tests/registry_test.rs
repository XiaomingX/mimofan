use mimofan_agent::*;
use mimofan_config::ProviderKind;

#[test]
fn deepseek_v4_pro_alias_stays_deepseek_by_default() {
    let registry = ModelRegistry::default();
    let resolved = registry.resolve(Some("deepseek-v4-pro"), None);

    assert_eq!(resolved.resolved.provider, ProviderKind::XiaomiMimo);
    assert_eq!(resolved.resolved.id, "deepseek-v4-pro");
}

#[test]
fn xiaomi_mimo_provider_hint_preserves_explicit_model_id_case() {
    let registry = ModelRegistry::default();
    let resolved = registry.resolve(Some("Qwen/Qwen3-Coder"), Some(ProviderKind::XiaomiMimo));

    assert_eq!(resolved.resolved.provider, ProviderKind::XiaomiMimo);
    assert_eq!(resolved.resolved.id, "Qwen/Qwen3-Coder");
    assert!(!resolved.used_fallback);
}

#[test]
fn xiaomi_mimo_tts_aliases_resolve_when_provider_hinted() {
    let registry = ModelRegistry::default();
    let resolved = registry.resolve(Some("tts"), Some(ProviderKind::XiaomiMimo));
    assert_eq!(resolved.resolved.provider, ProviderKind::XiaomiMimo);
    assert_eq!(resolved.resolved.id, "mimo-v2.5-tts");
    assert!(!resolved.resolved.supports_tools);
    assert!(!resolved.resolved.supports_reasoning);

    let resolved = registry.resolve(Some("voice-design"), Some(ProviderKind::XiaomiMimo));
    assert_eq!(resolved.resolved.id, "mimo-v2.5-tts-voicedesign");

    let resolved = registry.resolve(Some("voiceclone"), Some(ProviderKind::XiaomiMimo));
    assert_eq!(resolved.resolved.id, "mimo-v2.5-tts-voiceclone");
}

#[test]
fn xiaomi_mimo_chat_aliases_resolve_when_provider_hinted() {
    let registry = ModelRegistry::default();

    let resolved = registry.resolve(Some("omni"), Some(ProviderKind::XiaomiMimo));
    assert_eq!(resolved.resolved.provider, ProviderKind::XiaomiMimo);
    assert_eq!(resolved.resolved.id, "mimo-v2.5");
    assert!(resolved.resolved.supports_tools);
}

#[test]
fn xiaomi_mimo_provider_hint_preserves_custom_model_id() {
    let registry = ModelRegistry::default();
    let resolved =
        registry.resolve(Some("account-custom-mimo"), Some(ProviderKind::XiaomiMimo));

    assert_eq!(resolved.resolved.provider, ProviderKind::XiaomiMimo);
    assert_eq!(resolved.resolved.id, "account-custom-mimo");
    assert!(!resolved.used_fallback);
}

#[test]
fn xiaomi_mimo_provider_hint_does_not_reclassify_openrouter_model_id() {
    let registry = ModelRegistry::default();
    let resolved = registry.resolve(
        Some("deepseek/deepseek-v4-pro"),
        Some(ProviderKind::XiaomiMimo),
    );

    assert_eq!(resolved.resolved.provider, ProviderKind::XiaomiMimo);
    assert_eq!(resolved.resolved.id, "deepseek/deepseek-v4-pro");
    assert!(!resolved.used_fallback);
}

#[test]
fn first_party_recent_provider_models_are_listed() {
    let registry = ModelRegistry::default();
    let models = registry.list();

    for (provider, id) in [(ProviderKind::XiaomiMimo, "GLM-5.2")] {
        assert!(
            models
                .iter()
                .any(|model| model.provider == provider && model.id == id),
            "expected {provider:?} model {id} in registry"
        );
    }
}

#[test]
fn zai_direct_models_resolve_when_provider_hinted() {
    let registry = ModelRegistry::default();

    // model_matches checks aliases too; openrouter entries like
    // "z-ai/glm-5.1" have lowercase aliases that match the
    // normalized request, and they appear before direct entries.
    for (alias, expected) in [("GLM-5.2", "z-ai/glm-5.2")] {
        let resolved = registry.resolve(Some(alias), Some(ProviderKind::XiaomiMimo));

        assert_eq!(resolved.resolved.provider, ProviderKind::XiaomiMimo);
        assert_eq!(resolved.resolved.id, expected);
        assert!(!resolved.used_fallback);
        assert!(resolved.resolved.supports_tools);
        assert!(resolved.resolved.supports_reasoning);
    }
}

#[test]
fn preserves_requested_model_casing_for_third_party_providers() {
    let registry = ModelRegistry::default();
    let resolved = registry.resolve(Some("DeepSeek-V4-Pro"), None);

    assert_eq!(resolved.resolved.provider, ProviderKind::XiaomiMimo);
    assert_eq!(resolved.resolved.id, "DeepSeek-V4-Pro");
}

#[test]
fn registry_casing_takes_priority_over_requested_casing_with_provider_hint() {
    let registry = ModelRegistry::default();
    let resolved = registry.resolve(Some("DeepSeek-V4-Pro"), Some(ProviderKind::XiaomiMimo));

    assert_eq!(resolved.resolved.provider, ProviderKind::XiaomiMimo);
    // Registry's canonical id is used even when user provides different casing
    assert_eq!(resolved.resolved.id, "deepseek-v4-pro");
}

#[test]
fn preserves_requested_model_casing_without_surrounding_whitespace() {
    let registry = ModelRegistry::default();
    let resolved = registry.resolve(Some("  DeepSeek-V4-Pro  "), None);

    assert_eq!(resolved.resolved.provider, ProviderKind::XiaomiMimo);
    assert_eq!(resolved.resolved.id, "DeepSeek-V4-Pro");
}

#[test]
fn alias_match_does_not_override_requested_casing() {
    let registry = ModelRegistry::default();
    let resolved = registry.resolve(Some("deepseek-reasoner"), None);

    assert_eq!(resolved.resolved.provider, ProviderKind::XiaomiMimo);
    // alias `deepseek-reasoner` now resolves to the fully-qualified `deepseek-ai/deepseek-v4-flash`
    assert_eq!(resolved.resolved.id, "deepseek-ai/deepseek-v4-flash");
}

#[test]
fn unknown_model_is_passed_through_with_provider_hint() {
    let registry = ModelRegistry::default();
    let resolved =
        registry.resolve(Some("not-registered-model"), Some(ProviderKind::XiaomiMimo));

    assert_eq!(resolved.resolved.provider, ProviderKind::XiaomiMimo);
    assert_eq!(resolved.resolved.id, "not-registered-model");
    assert!(!resolved.used_fallback);
}

#[test]
fn explicit_slash_model_id_is_passed_through() {
    let registry = ModelRegistry::default();
    let resolved = registry.resolve(
        Some("custom-org/custom-model"),
        Some(ProviderKind::XiaomiMimo),
    );

    assert_eq!(resolved.resolved.provider, ProviderKind::XiaomiMimo);
    assert_eq!(resolved.resolved.id, "custom-org/custom-model");
    assert!(!resolved.used_fallback);
}

#[test]
fn default_resolve_without_hint_returns_first_model() {
    let registry = ModelRegistry::default();
    let resolved = registry.resolve(None, None);

    assert_eq!(resolved.resolved.provider, ProviderKind::XiaomiMimo);
    assert_eq!(resolved.resolved.id, "deepseek-v4-pro");
    assert!(resolved.used_fallback);
}
