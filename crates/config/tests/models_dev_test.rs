//! Externalized integration tests for `mimofan_config::models_dev`.
//!
//! Relocated verbatim from `crates/config/src/models_dev.rs`. Only the
//! `#[cfg(test)] mod tests` wrapper and the `use super::*` import were replaced
//! with the public-API imports below; no test logic or assertion changed.

use mimofan_config::models_dev::*;
use mimofan_config::route::ModelId;

const GLM_FIXTURE: &str = r#"{
      "models": {
        "zhipuai/glm-5.2": {
          "id": "zhipuai/glm-5.2",
          "name": "GLM-5.2",
          "family": "glm",
          "reasoning": true,
          "tool_call": true,
          "structured_output": true,
          "modalities": { "input": ["text"], "output": ["text"] },
          "limit": { "context": 1000000, "output": 131072 },
          "open_weights": true
        }
      },
      "providers": {
        "zhipuai": {
          "id": "zhipuai",
          "name": "Zhipu AI",
          "api": "https://open.bigmodel.cn/api/paas/v4",
          "npm": "@ai-sdk/openai-compatible",
          "env": ["ZHIPU_API_KEY"],
          "models": {
            "glm-5.2": {
              "id": "glm-5.2",
              "name": "GLM-5.2",
              "family": "glm",
              "reasoning": true,
              "reasoning_options": [{ "type": "effort", "values": ["high", "max"] }],
              "tool_call": true,
              "structured_output": true,
              "modalities": { "input": ["text"], "output": ["text"] },
              "limit": { "context": 1000000, "output": 131072 },
              "cost": { "input": 1.4, "output": 4.4, "cache_read": 0.26 }
            }
          }
        },
        "zai": {
          "id": "zai",
          "name": "Z.AI",
          "api": "https://api.z.ai/api/paas/v4",
          "npm": "@ai-sdk/openai-compatible",
          "env": ["ZHIPU_API_KEY"],
          "models": {
            "glm-5.2": {
              "id": "glm-5.2",
              "family": "glm",
              "reasoning": true,
              "tool_call": true,
              "modalities": { "input": ["text"], "output": ["text"] },
              "cost": { "input": 1.4, "output": 4.4 }
            }
          }
        }
      }
    }"#;

#[test]
fn parses_models_dev_catalog_layers_without_joining_by_prefix() {
    let catalog = ModelsDevCatalog::parse_json(GLM_FIXTURE).expect("fixture parses");

    let canonical = catalog.model("zhipuai/glm-5.2").expect("canonical model");
    assert_eq!(canonical.family.as_deref(), Some("glm"));
    assert_eq!(
        canonical.limit.as_ref().and_then(|limit| limit.context),
        Some(1_000_000)
    );
    assert!(canonical.supports_text_chat());

    let provider = catalog.provider("zhipuai").expect("provider");
    assert_eq!(
        provider.api.as_deref(),
        Some("https://open.bigmodel.cn/api/paas/v4")
    );
    assert_eq!(provider.npm.as_deref(), Some("@ai-sdk/openai-compatible"));
    assert_eq!(provider.env, ["ZHIPU_API_KEY"]);

    let offering = catalog
        .provider_model("zhipuai", "glm-5.2")
        .expect("provider model");
    assert_eq!(offering.id, "glm-5.2");
    assert_eq!(offering.reasoning, Some(true));
    assert_eq!(
        offering.cost.as_ref().and_then(|cost| cost.cache_read),
        Some(0.26)
    );
    assert!(offering.supports_text_chat());
    assert_eq!(
        offering.base_model, None,
        "generated JSON does not prove a canonical join"
    );

    let route_offering = catalog
        .provider_offering("zhipuai", "glm-5.2")
        .expect("route offering");
    assert_eq!(route_offering.limits.context_tokens, Some(1_000_000));
    assert_eq!(route_offering.limits.output_tokens, Some(131_072));
}

#[test]
fn provider_offering_preserves_wire_id_without_inferred_canonical_model() {
    let catalog = ModelsDevCatalog::parse_json(GLM_FIXTURE).expect("fixture parses");
    let offering = catalog
        .provider_offering("zai", "glm-5.2")
        .expect("offering");

    assert_eq!(offering.provider.as_str(), "zai");
    assert_eq!(offering.wire_model_id.as_str(), "glm-5.2");
    assert_eq!(offering.canonical_model, None);
    assert_eq!(offering.endpoint_key, "chat");
}

#[test]
fn provider_offering_uses_explicit_base_model_when_present() {
    let raw = r#"{
          "providers": {
            "openrouter": {
              "id": "openrouter",
              "models": {
                "z-ai/glm-5.2": {
                  "id": "z-ai/glm-5.2",
                  "base_model": "zhipuai/glm-5.2"
                }
              }
            }
          }
        }"#;
    let catalog = ModelsDevCatalog::parse_json(raw).expect("fixture parses");
    let offering = catalog
        .provider_offering("openrouter", "z-ai/glm-5.2")
        .expect("offering");

    assert_eq!(
        offering.canonical_model.as_ref().map(ModelId::as_str),
        Some("zhipuai/glm-5.2")
    );
    assert_eq!(offering.wire_model_id.as_str(), "z-ai/glm-5.2");
}

#[test]
fn provider_offerings_emit_chat_rows_and_skip_non_text_outputs() {
    let raw = r#"{
          "providers": {
            "zai": {
              "models": {
                "glm-5.2": {
                  "id": "glm-5.2",
                  "base_model": "zhipuai/glm-5.2",
                  "default": true,
                  "modalities": { "input": ["text"], "output": ["text"] }
                },
                "glm-voice": {
                  "id": "glm-voice",
                  "modalities": { "input": ["text"], "output": ["audio"] }
                }
              }
            }
          }
        }"#;
    let catalog = ModelsDevCatalog::parse_json(raw).expect("fixture parses");
    let offerings = catalog
        .provider_offerings("zai")
        .expect("provider offerings");

    assert_eq!(offerings.len(), 1);
    assert_eq!(offerings[0].provider.as_str(), "zai");
    assert_eq!(offerings[0].wire_model_id.as_str(), "glm-5.2");
    assert_eq!(
        offerings[0].canonical_model.as_ref().map(ModelId::as_str),
        Some("zhipuai/glm-5.2")
    );
    assert!(offerings[0].default_for_provider);
}

#[test]
fn non_text_output_is_not_a_chat_model() {
    let model = ModelsDevProviderModel {
        id: "mimo-v2.5-tts".to_string(),
        modalities: Some(ModelsDevModalities {
            input: vec!["text".to_string()],
            output: vec!["audio".to_string()],
        }),
        ..Default::default()
    };

    assert!(!model.supports_text_chat());
}

#[test]
fn empty_modalities_struct_is_chat_capable() {
    // `"modalities": {}` deserializes to Some(empty); it must default to
    // chat-capable just like absent modality metadata (the None branch),
    // otherwise rows from incomplete snapshots are silently dropped.
    let provider_model = ModelsDevProviderModel {
        modalities: Some(ModelsDevModalities::default()),
        ..Default::default()
    };
    assert!(provider_model.supports_text_chat());

    let canonical = ModelsDevModel {
        modalities: Some(ModelsDevModalities::default()),
        ..Default::default()
    };
    assert!(canonical.supports_text_chat());

    // A list populated with only non-text entries still excludes the row.
    let audio_only = ModelsDevProviderModel {
        modalities: Some(ModelsDevModalities {
            input: vec!["text".to_string()],
            output: vec!["audio".to_string()],
        }),
        ..Default::default()
    };
    assert!(!audio_only.supports_text_chat());
}

#[test]
fn provider_offerings_keep_rows_with_empty_modalities_object() {
    // End-to-end guard for the empty-modalities case at the offering layer:
    // a custom/local provider row with `"modalities": {}` must still emit a
    // chat offering rather than being filtered out of route resolution.
    let raw = r#"{
          "providers": {
            "custom": {
              "models": {
                "house-model": { "id": "house-model", "modalities": {} }
              }
            }
          }
        }"#;
    let catalog = ModelsDevCatalog::parse_json(raw).expect("fixture parses");
    let offerings = catalog
        .provider_offerings("custom")
        .expect("provider offerings");

    assert_eq!(offerings.len(), 1);
    assert_eq!(offerings[0].wire_model_id.as_str(), "house-model");
    // `id` was omitted on the provider row → effective id is the catalog key.
    assert_eq!(offerings[0].provider.as_str(), "custom");
}
