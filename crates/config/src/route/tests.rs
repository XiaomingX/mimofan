//! Behavior tests for the route foundation (#2608 / #3084 / #3384).

use super::RequestProtocol;
use super::descriptor::ProviderDescriptor;
use super::errors::RouteError;
use super::ids::{LogicalModelRef, ModelId, NamespaceHint, ProviderId, WireModelId};
use super::resolver::{RouteRequest, RouteResolver};
use crate::ProviderKind;
use crate::models_dev::ModelsDevCatalog;

/// Build a request with only an explicit provider + a model selector string.
fn req(provider: Option<ProviderKind>, model: Option<&str>) -> RouteRequest {
    RouteRequest {
        explicit_provider: provider,
        model_selector: model.map(LogicalModelRef::from),
        saved_provider_model: None,
        base_url_override: None,
    }
}

fn models_dev_route_resolver() -> RouteResolver {
    let raw = r#"{
      "providers": {
        "xiaomi-mimo": {
          "models": {
            "glm-5.2": {
              "id": "glm-5.2",
              "base_model": "zhipuai/glm-5.2",
              "default": true,
              "modalities": { "input": ["text"], "output": ["text"] },
              "limit": { "context": 1000000, "input": 900000, "output": 131072 }
            }
          }
        },
        "custom": {
          "models": {
            "z-ai/glm-5.2": {
              "id": "z-ai/glm-5.2",
              "base_model": "zhipuai/glm-5.2",
              "modalities": { "input": ["text"], "output": ["text"] },
              "limit": { "context": 128000, "output": 32768 }
            }
          }
        }
      }
    }"#;
    let catalog = ModelsDevCatalog::parse_json(raw).expect("Models.dev fixture parses");
    let mut offerings = catalog
        .provider_offerings("xiaomi-mimo")
        .expect("xiaomi-mimo provider offerings");
    offerings.extend(
        catalog
            .provider_offerings("custom")
            .expect("custom provider offerings"),
    );
    RouteResolver::from_offerings(offerings)
}

#[test]
fn provider_id_from_kind_uses_canonical_id() {
    assert_eq!(
        ProviderId::from_kind(ProviderKind::XiaomiMimo).as_str(),
        "xiaomi-mimo"
    );
    assert_eq!(
        ProviderId::from_kind(ProviderKind::Custom).as_str(),
        "custom"
    );
}

#[test]
fn model_id_and_wire_model_id_are_distinct_types() {
    // This test asserts the values; the *type* distinction is enforced by the
    // compiler: a function taking `WireModelId` rejects a `ModelId` argument.
    let canonical = ModelId::from("deepseek-v4-pro");
    let wire = WireModelId::from("deepseek-ai/DeepSeek-V4-Pro");
    assert_eq!(canonical.as_str(), "deepseek-v4-pro");
    assert_eq!(wire.as_str(), "deepseek-ai/DeepSeek-V4-Pro");
}

#[test]
fn logical_model_ref_auto_is_sentinel() {
    assert!(LogicalModelRef::from("auto").is_auto());
    assert!(!LogicalModelRef::from("deepseek-v4-pro").is_auto());
}

#[test]
fn logical_model_ref_namespace_hint_parses_curated_prefixes() {
    let cases = [
        ("deepseek-ai/DeepSeek-V4-Pro", NamespaceHint::DeepseekAi),
        ("deepseek/deepseek-v4-pro", NamespaceHint::Deepseek),
        ("anthropic/claude-foo", NamespaceHint::Anthropic),
        ("openai/gpt-foo", NamespaceHint::Openai),
        ("qwen/qwen-foo", NamespaceHint::Qwen),
    ];
    for (raw, expected) in cases {
        assert_eq!(
            LogicalModelRef::from(raw).namespace_hint(),
            Some(expected),
            "{raw} should parse to {expected:?}"
        );
    }
    assert_eq!(LogicalModelRef::from("plain-model").namespace_hint(), None);
    assert_eq!(LogicalModelRef::from("auto").namespace_hint(), None);
}

/// By construction there is NO path from a namespace prefix to a provider.
///
/// This is enforced by the *absence* of any `From<NamespaceHint>` /
/// `From<LogicalModelRef>` for `ProviderId`. The following lines are the
/// canonical way to mint a `ProviderId` and demonstrate the only supported
/// source is an explicit `ProviderKind`, never a parsed prefix.
#[test]
fn no_namespace_hint_or_logical_ref_to_provider_id_conversion() {
    let hint = LogicalModelRef::from("deepseek-ai/DeepSeek-V4-Pro").namespace_hint();
    assert_eq!(hint, Some(NamespaceHint::DeepseekAi));
    // A ProviderId may ONLY be built from an explicit ProviderKind or string,
    // never derived from the hint above. (If a `From<NamespaceHint>` for
    // `ProviderId` were ever added, this seam would silently break #2608.)
    let provider = ProviderId::from_kind(ProviderKind::Custom);
    assert_eq!(provider.as_str(), "custom");
}

#[test]
fn newtypes_serialize_transparently() {
    let id = ProviderId::from("deepseek");
    assert_eq!(serde_json::to_string(&id).unwrap(), "\"deepseek\"");
    let wire = WireModelId::from("deepseek-ai/DeepSeek-V4-Pro");
    assert_eq!(
        serde_json::to_string(&wire).unwrap(),
        "\"deepseek-ai/DeepSeek-V4-Pro\""
    );
}

#[test]
fn descriptor_for_every_kind_has_nonempty_transport_facts() {
    for kind in ProviderKind::ALL {
        let d = ProviderDescriptor::for_kind(kind);
        assert!(!d.id().as_str().is_empty(), "{kind:?} id empty");
        assert!(
            !d.default_base_url().is_empty(),
            "{kind:?} default_base_url empty"
        );
        assert!(
            !d.default_wire_model().as_str().is_empty(),
            "{kind:?} default_wire_model empty"
        );
        // protocol() always yields a RequestProtocol; calling it must not panic.
        let _: RequestProtocol = d.protocol();
    }
}

#[test]
fn descriptor_protocol_matches_provider_wire() {
    for kind in ProviderKind::ALL {
        let d = ProviderDescriptor::for_kind(kind);
        assert_eq!(
            d.protocol(),
            kind.provider().wire(),
            "{kind:?} protocol must equal provider().wire()"
        );
        let expected = RequestProtocol::ChatCompletions;
        assert_eq!(d.protocol(), expected, "{kind:?} protocol mismatch");
    }
}

#[test]
fn resolver_explicit_provider_scoped_model_maps_to_wire_id() {
    let r = RouteResolver::new();
    let out = r
        .resolve(&req(Some(ProviderKind::XiaomiMimo), Some("mimo-v2.5-pro")))
        .expect("should resolve");
    assert_eq!(out.provider_kind, ProviderKind::XiaomiMimo);
    assert_eq!(out.wire_model_id.as_str(), "mimo-v2.5-pro");
    assert_eq!(
        out.canonical_model.as_ref().map(ModelId::as_str),
        None,
        "bundled mimo-v2.5-pro has no base_model so canonical_model stays None"
    );
}

#[test]
fn resolver_aggregator_preserves_prefixed_wire_id_without_inferring_deepseek() {
    let r = RouteResolver::new();
    let out = r
        .resolve(&req(
            Some(ProviderKind::Custom),
            Some("deepseek-ai/DeepSeek-V4-Pro"),
        ))
        .expect("aggregator should resolve");
    // Provider stays Custom, NOT XiaomiMimo, despite the deepseek-ai/ prefix.
    assert_eq!(out.provider_kind, ProviderKind::Custom);
    assert_ne!(out.provider_kind, ProviderKind::XiaomiMimo);
    // Wire id preserved verbatim.
    assert_eq!(out.wire_model_id.as_str(), "deepseek-ai/DeepSeek-V4-Pro");
}

#[test]
fn resolver_openrouter_keeps_provider_for_every_namespace_prefix() {
    let r = RouteResolver::new();
    let prefixes = [
        "deepseek-ai/DeepSeek-V4-Pro",
        "deepseek/deepseek-v4-pro",
        "anthropic/claude-foo",
        "openai/gpt-foo",
        "qwen/qwen-foo",
    ];
    for raw in prefixes {
        let selector = LogicalModelRef::from(raw);
        // The selector DOES carry a namespace hint...
        assert!(
            selector.namespace_hint().is_some(),
            "{raw} should have a namespace hint"
        );
        let out = r
            .resolve(&req(Some(ProviderKind::Custom), Some(raw)))
            .unwrap_or_else(|e| panic!("{raw} should resolve on custom: {e}"));
        // ...but the provider stays Custom regardless.
        assert_eq!(
            out.provider_kind,
            ProviderKind::Custom,
            "{raw} must not change provider"
        );
        assert_eq!(out.wire_model_id.as_str(), raw, "{raw} wire id verbatim");
    }
}

#[test]
fn resolver_auto_is_sentinel_not_literal_model() {
    let r = RouteResolver::new();
    let out = r
        .resolve(&req(Some(ProviderKind::XiaomiMimo), Some("auto")))
        .expect("auto should resolve");
    // The logical selector is the auto sentinel...
    assert!(out.logical_model.is_auto());
    // ...and "auto" is NOT put on the wire as a literal model.
    assert_ne!(out.wire_model_id.as_str(), "auto");
    assert_eq!(out.wire_model_id.as_str(), "mimo-v2.5-pro");
}

#[test]
fn resolver_can_use_models_dev_offering_for_provider_scoped_route() {
    let r = models_dev_route_resolver();
    let out = r
        .resolve(&req(Some(ProviderKind::XiaomiMimo), Some("glm-5.2")))
        .expect("Models.dev-backed XiaomiMiMo route should resolve");

    assert_eq!(out.provider_kind, ProviderKind::XiaomiMimo);
    assert_eq!(out.provider_id.as_str(), "xiaomi-mimo");
    assert_eq!(out.wire_model_id.as_str(), "glm-5.2");
    assert_eq!(
        out.canonical_model.as_ref().map(ModelId::as_str),
        Some("zhipuai/glm-5.2")
    );
}

#[test]
fn resolver_auto_uses_models_dev_default_offering_when_available() {
    let r = models_dev_route_resolver();
    let out = r
        .resolve(&req(Some(ProviderKind::XiaomiMimo), Some("auto")))
        .expect("auto should resolve through catalog default");

    assert!(out.logical_model.is_auto());
    assert_eq!(
        out.wire_model_id.as_str(),
        "glm-5.2",
        "catalog default should win over the built-in XiaomiMiMo spelling"
    );
    assert_eq!(
        out.canonical_model.as_ref().map(ModelId::as_str),
        Some("zhipuai/glm-5.2")
    );
}

#[test]
fn resolver_auto_falls_back_to_descriptor_default_without_catalog_default() {
    // XiaomiMiMo offerings exist in the catalog snapshot but none is marked
    // `default: true`. `auto` must then fall back to the provider descriptor's
    // built-in default wire model rather than picking an arbitrary catalog row.
    let raw = r#"{
      "providers": {
        "xiaomi-mimo": {
          "models": {
            "glm-5-turbo": {
              "id": "glm-5-turbo",
              "modalities": { "input": ["text"], "output": ["text"] }
            }
          }
        }
      }
    }"#;
    let catalog = ModelsDevCatalog::parse_json(raw).expect("Models.dev fixture parses");
    let offerings = catalog
        .provider_offerings("xiaomi-mimo")
        .expect("xiaomi-mimo provider offerings");
    let r = RouteResolver::from_offerings(offerings);

    let out = r
        .resolve(&req(Some(ProviderKind::XiaomiMimo), Some("auto")))
        .expect("auto should resolve to the descriptor default");

    assert!(out.logical_model.is_auto());
    assert_eq!(
        out.wire_model_id.as_str(),
        "mimo-v2.5-pro",
        "no catalog default -> descriptor built-in default wins"
    );
    assert_eq!(
        out.canonical_model, None,
        "descriptor fallback carries no catalog canonical link"
    );
}

#[test]
fn resolver_models_dev_prefixed_wire_id_stays_inside_provider_scope() {
    let r = models_dev_route_resolver();
    let out = r
        .resolve(&req(Some(ProviderKind::Custom), Some("z-ai/glm-5.2")))
        .expect("Custom Models.dev row should resolve");

    assert_eq!(out.provider_kind, ProviderKind::Custom);
    assert_ne!(out.provider_kind, ProviderKind::XiaomiMimo);
    assert_eq!(out.wire_model_id.as_str(), "z-ai/glm-5.2");
    assert_eq!(
        out.canonical_model.as_ref().map(ModelId::as_str),
        Some("zhipuai/glm-5.2")
    );
}

#[test]
fn resolver_carries_models_dev_limits_into_ready_candidate() {
    let r = models_dev_route_resolver();
    let out = r
        .resolve(&req(Some(ProviderKind::XiaomiMimo), Some("glm-5.2")))
        .expect("XiaomiMiMo Models.dev row should resolve");

    assert_eq!(out.limits.context_tokens, Some(1_000_000));
    assert_eq!(out.limits.input_tokens, Some(900_000));
    assert_eq!(out.limits.output_tokens, Some(131_072));
    assert!(out.limits.has_known_limit());
}

#[test]
fn resolver_keeps_limits_provider_scoped_for_same_canonical_model() {
    let r = models_dev_route_resolver();
    let direct = r
        .resolve(&req(Some(ProviderKind::XiaomiMimo), Some("glm-5.2")))
        .expect("direct XiaomiMiMo route should resolve");
    let hosted = r
        .resolve(&req(Some(ProviderKind::Custom), Some("z-ai/glm-5.2")))
        .expect("hosted Custom route should resolve");

    assert_eq!(
        direct.canonical_model.as_ref().map(ModelId::as_str),
        hosted.canonical_model.as_ref().map(ModelId::as_str)
    );
    assert_eq!(direct.limits.context_tokens, Some(1_000_000));
    assert_eq!(hosted.limits.context_tokens, Some(128_000));
    assert_eq!(hosted.limits.output_tokens, Some(32_768));
}

#[test]
fn resolver_custom_endpoint_allows_namespaced_selector_for_strict_provider() {
    let r = RouteResolver::new();
    let request = RouteRequest {
        explicit_provider: Some(ProviderKind::XiaomiMimo),
        model_selector: Some(LogicalModelRef::from("vendor/custom-coder")),
        saved_provider_model: None,
        base_url_override: Some("https://example.local/v1".to_string()),
    };
    let out = r
        .resolve(&request)
        .expect("custom endpoint should defer model validation upstream");
    assert_eq!(out.provider_kind, ProviderKind::XiaomiMimo);
    assert_eq!(out.wire_model_id.as_str(), "vendor/custom-coder");
    assert_eq!(out.endpoint.base_url, "https://example.local/v1");
}

#[test]
fn resolver_explicit_custom_with_base_url_override_passes_model_through_verbatim() {
    // #1519: an explicit `Custom` provider with a base_url override resolves via
    // the LocalOrCustom pass-through, preserving even a namespaced selector as
    // the verbatim wire id and binding the override endpoint + Chat Completions.
    let r = RouteResolver::new();
    let request = RouteRequest {
        explicit_provider: Some(ProviderKind::Custom),
        model_selector: Some(LogicalModelRef::from("vendor/custom-model-v1")),
        saved_provider_model: None,
        base_url_override: Some("https://api.example.com/v1".to_string()),
    };
    let out = r
        .resolve(&request)
        .expect("custom provider should resolve via pass-through");
    assert_eq!(out.provider_kind, ProviderKind::Custom);
    assert_eq!(out.provider_id.as_str(), "custom");
    assert_eq!(out.wire_model_id.as_str(), "vendor/custom-model-v1");
    assert_eq!(out.endpoint.base_url, "https://api.example.com/v1");
    assert_eq!(out.protocol, crate::route::RequestProtocol::ChatCompletions);
    assert!(out.validation.ok);
    assert!(out.validation.messages.is_empty());
}

// ---------------------------------------------------------------------------
// #3385: the DEFAULT resolver now sources the bundled Models.dev catalog asset,
// so real provider/model facts (context windows) reach candidates.
// ---------------------------------------------------------------------------

#[test]
fn default_resolver_yields_real_facts_from_bundled_catalog() {
    let r = RouteResolver::new();

    // A MiMo v2.5 Pro row resolves to a real, non-default context window —
    // proof the bundled asset feeds the default resolver with real facts.
    let mimo = r
        .resolve(&req(Some(ProviderKind::XiaomiMimo), Some("mimo-v2.5-pro")))
        .expect("mimo-v2.5-pro should resolve from the bundled catalog");
    assert_eq!(mimo.provider_kind, ProviderKind::XiaomiMimo);
    assert_eq!(mimo.wire_model_id.as_str(), "mimo-v2.5-pro");
    assert_eq!(
        mimo.limits.context_tokens,
        Some(1_000_000),
        "mimo-v2.5-pro must carry its real context window, not the unknown default"
    );
    assert_eq!(mimo.limits.output_tokens, Some(131_072));
    assert!(mimo.limits.has_known_limit());

    // A MiMo v2.5 row likewise resolves with its real window.
    let mimo_lite = r
        .resolve(&req(Some(ProviderKind::XiaomiMimo), Some("mimo-v2.5")))
        .expect("mimo-v2.5 should resolve from the bundled catalog");
    assert_eq!(mimo_lite.limits.context_tokens, Some(1_000_000));
    assert_eq!(mimo_lite.limits.output_tokens, Some(131_072));
}

#[test]
fn resolver_deepseek_none_selector_uses_default_wire_id() {
    let r = RouteResolver::new();
    let out = r
        .resolve(&req(Some(ProviderKind::XiaomiMimo), None))
        .expect("none selector should use provider default");
    assert_eq!(out.provider_kind, ProviderKind::XiaomiMimo);
    assert_eq!(out.wire_model_id.as_str(), "mimo-v2.5-pro");
}

#[test]
fn resolver_empty_string_selector_is_empty_model_error() {
    let r = RouteResolver::new();
    let out = r.resolve(&req(Some(ProviderKind::XiaomiMimo), Some("")));
    assert!(matches!(out, Err(RouteError::EmptyModel)));
}

#[test]
fn resolver_empty_saved_provider_model_is_empty_model_error() {
    // An empty selector from the saved-model fallback must be rejected too, not
    // just an empty explicit selector (the guard covers every selector source).
    let r = RouteResolver::new();
    let request = RouteRequest {
        explicit_provider: Some(ProviderKind::XiaomiMimo),
        model_selector: None,
        saved_provider_model: Some(WireModelId::from("")),
        base_url_override: None,
    };
    assert!(matches!(r.resolve(&request), Err(RouteError::EmptyModel)));
}

#[test]
fn resolver_passthrough_provider_preserves_custom_id_verbatim() {
    let r = RouteResolver::new();
    let out = r
        .resolve(&req(Some(ProviderKind::Custom), Some("my-local:7b")))
        .expect("local passthrough should resolve");
    assert_eq!(out.provider_kind, ProviderKind::Custom);
    assert_eq!(out.wire_model_id.as_str(), "my-local:7b");
    assert_eq!(out.limits, Default::default());
    assert!(out.validation.ok);
}

#[test]
fn resolved_candidate_serializes_secret_free() {
    let r = RouteResolver::new();
    // Cover a direct, an aggregator, and a local/passthrough route.
    let candidates = [
        r.resolve(&req(Some(ProviderKind::XiaomiMimo), Some("mimo-v2.5-pro")))
            .expect("direct resolves"),
        r.resolve(&req(
            Some(ProviderKind::Custom),
            Some("deepseek-ai/DeepSeek-V4-Pro"),
        ))
        .expect("aggregator resolves"),
        r.resolve(&req(Some(ProviderKind::Custom), Some("my-local:7b")))
            .expect("local resolves"),
    ];
    for out in candidates {
        let json = serde_json::to_string(&out).expect("candidate serializes");
        // Carries provider/model/wire/protocol/auth-source class.
        assert!(json.contains("provider_id"), "{json}");
        assert!(json.contains("provider_kind"), "{json}");
        assert!(json.contains("wire_model_id"), "{json}");
        assert!(json.contains("protocol"), "{json}");
        assert!(json.contains("auth"), "{json}");
        // Never any secret/api-key material.
        let lower = json.to_lowercase();
        assert!(!lower.contains("api_key"), "leaked api_key: {json}");
        assert!(!lower.contains("apikey"), "leaked apikey: {json}");
        assert!(!lower.contains("secret_id"), "leaked secret_id: {json}");
        assert!(!lower.contains("password"), "leaked password: {json}");
        assert!(!lower.contains("bearer"), "leaked bearer: {json}");
        assert!(
            !lower.contains("authorization"),
            "leaked authorization: {json}"
        );
    }
}

#[test]
fn resolver_protocol_matches_descriptor_for_every_provider() {
    let r = RouteResolver::new();
    for kind in ProviderKind::ALL {
        // Use each provider's own default wire id as the selector so strict
        // direct providers do not reject; this exercises the resolver across
        // the whole provider set.
        let default_wire = ProviderDescriptor::for_kind(kind).default_wire_model();
        let request = req(Some(kind), Some(default_wire.as_str()));
        let out = r
            .resolve(&request)
            .unwrap_or_else(|e| panic!("{kind:?} should resolve its own default: {e}"));
        assert_eq!(
            out.protocol,
            ProviderDescriptor::for_kind(kind).protocol(),
            "{kind:?} candidate protocol must match descriptor"
        );
        assert_eq!(
            out.endpoint.protocol, out.protocol,
            "{kind:?} endpoint protocol"
        );
    }
}

// ---------------------------------------------------------------------------
// #3085: honest pricing on resolved candidates.
// ---------------------------------------------------------------------------

/// A resolver whose single offering is a priced catalog row, projected
/// through the wired `CatalogOffering::to_offering` pricing seam.
fn priced_xiaomi_mimo_resolver() -> RouteResolver {
    use crate::catalog::{CatalogOffering, CatalogSource};
    use crate::models_dev::ModelsDevCost;

    let priced = CatalogOffering {
        provider: "xiaomi-mimo".into(),
        wire_model_id: "mimo-v2.5-pro".into(),
        canonical_model: Some("mimo-v2.5-pro".into()),
        endpoint_key: "chat".into(),
        default_for_provider: true,
        cost: Some(ModelsDevCost {
            input: Some(0.28),
            output: Some(0.42),
            cache_read: Some(0.028),
            cache_write: None,
        }),
        source: CatalogSource::Bundled,
        ..Default::default()
    };
    RouteResolver::from_offerings(vec![priced.to_offering()])
}

#[test]
fn priced_offering_yields_token_pricing_sku() {
    use super::candidate::PricingSku;

    let r = priced_xiaomi_mimo_resolver();
    let out = r
        .resolve(&req(Some(ProviderKind::XiaomiMimo), Some("mimo-v2.5-pro")))
        .expect("priced XiaomiMiMo route should resolve");

    match out.pricing {
        Some(PricingSku::Token {
            input_per_mtok,
            output_per_mtok,
        }) => {
            assert_eq!(input_per_mtok, Some(0.28));
            assert_eq!(output_per_mtok, Some(0.42));
        }
        other => panic!("expected Some(Token), got {other:?}"),
    }
}

#[test]
fn unpriced_offering_stays_unknown() {
    use super::candidate::PricingSku;

    // The bundled seam (`RouteResolver::new`) carries no sourced cost, so a
    // matched offering must surface honest UnknownOrStale, never a fabricated
    // zero price (#2608 / #3085 honesty rule).
    let r = RouteResolver::new();
    let out = r
        .resolve(&req(Some(ProviderKind::XiaomiMimo), Some("mimo-v2.5-pro")))
        .expect("bundled XiaomiMiMo route should resolve");
    assert!(
        matches!(out.pricing, Some(PricingSku::UnknownOrStale)),
        "bundled offering carries no price -> UnknownOrStale, got {:?}",
        out.pricing
    );

    // A pass-through route with no matched offering is likewise unknown.
    let passthrough = r
        .resolve(&req(Some(ProviderKind::Custom), Some("my-local:7b")))
        .expect("local passthrough should resolve");
    assert!(matches!(
        passthrough.pricing,
        Some(PricingSku::UnknownOrStale)
    ));
}

// ---------------------------------------------------------------------------
// #1519: advisory insecure-http warning, loopback-exempt.
// ---------------------------------------------------------------------------

/// Build a request with an explicit base-URL override.
fn req_with_base(provider: ProviderKind, model: &str, base_url: &str) -> RouteRequest {
    RouteRequest {
        explicit_provider: Some(provider),
        model_selector: Some(LogicalModelRef::from(model)),
        saved_provider_model: None,
        base_url_override: Some(base_url.to_string()),
    }
}

#[test]
fn http_custom_endpoint_emits_insecure_warning() {
    let r = RouteResolver::new();
    let out = r
        .resolve(&req_with_base(
            ProviderKind::Custom,
            "gpt-whatever",
            "http://example.com/v1",
        ))
        .expect("custom http endpoint should still resolve");

    // Advisory only: the route stays usable.
    assert!(
        out.validation.ok,
        "insecure http is advisory, not a hard fail"
    );
    assert!(
        out.validation
            .messages
            .iter()
            .any(|m| m.contains("insecure http")),
        "expected an insecure-http advisory, got {:?}",
        out.validation.messages
    );
}

#[test]
fn loopback_http_endpoint_does_not_warn() {
    let r = RouteResolver::new();
    // localhost, 127.0.0.1, and ::1 are all loopback and must stay clean.
    for base in [
        "http://localhost:11434/v1",
        "http://127.0.0.1:8000/v1",
        "http://[::1]:8080/v1",
    ] {
        let out = r
            .resolve(&req_with_base(ProviderKind::Custom, "my-local:7b", base))
            .unwrap_or_else(|e| panic!("loopback route {base} should resolve: {e}"));
        assert!(out.validation.ok);
        assert!(
            out.validation.messages.is_empty(),
            "loopback {base} must not warn, got {:?}",
            out.validation.messages
        );
    }
}

#[test]
fn https_endpoint_has_no_warning() {
    let r = RouteResolver::new();
    let out = r
        .resolve(&req_with_base(
            ProviderKind::Custom,
            "gpt-whatever",
            "https://example.com/v1",
        ))
        .expect("https endpoint should resolve");
    assert!(out.validation.ok);
    assert!(
        out.validation.messages.is_empty(),
        "https must not warn, got {:?}",
        out.validation.messages
    );
}
