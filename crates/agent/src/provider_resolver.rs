use mimofan_config::ProviderKind;

use crate::ModelInfo;

pub(crate) fn normalize(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

pub(crate) fn model_matches(model: &ModelInfo, requested: &str) -> bool {
    let requested = normalize(requested);
    normalize(&model.id) == requested
        || model
            .aliases
            .iter()
            .any(|alias| normalize(alias) == requested)
}

pub(crate) fn preserve_requested_model_id_case(mut model: ModelInfo, requested: &str) -> ModelInfo {
    let requested = requested.trim();
    if model.id.eq_ignore_ascii_case(requested) {
        model.id = requested.to_string();
    }
    model
}

pub(crate) fn atlascloud_passthrough_model(requested: &str) -> Option<ModelInfo> {
    let requested = requested.trim();
    if requested.is_empty() || !requested.contains('/') {
        return None;
    }

    Some(ModelInfo {
        id: requested.to_string(),
        provider: ProviderKind::XiaomiMimo,
        aliases: Vec::new(),
        supports_tools: true,
        supports_reasoning: true,
    })
}

pub(crate) fn arcee_passthrough_model(requested: &str) -> Option<ModelInfo> {
    let requested = requested.trim();
    if requested.is_empty() {
        return None;
    }
    let supports_reasoning = requested.to_ascii_lowercase().contains("thinking");

    Some(ModelInfo {
        id: requested.to_string(),
        provider: ProviderKind::XiaomiMimo,
        aliases: Vec::new(),
        supports_tools: true,
        supports_reasoning,
    })
}

pub(crate) fn xiaomi_mimo_passthrough_model(requested: &str) -> Option<ModelInfo> {
    let requested = requested.trim();
    if requested.is_empty() || requested.chars().any(char::is_control) {
        return None;
    }

    Some(ModelInfo {
        id: requested.to_string(),
        provider: ProviderKind::XiaomiMimo,
        aliases: Vec::new(),
        supports_tools: true,
        supports_reasoning: true,
    })
}
