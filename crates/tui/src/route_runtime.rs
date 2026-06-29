use mimofan_config::route::{
    LogicalModelRef, ReadyRouteCandidate, RouteRequest, RouteResolver, WireModelId,
};

use crate::config::{ApiProvider, Config, DEFAULT_NVIDIA_NIM_BASE_URL};

#[derive(Debug, Clone)]
pub(crate) struct ResolvedRuntimeRoute {
    pub(crate) candidate: ReadyRouteCandidate,
    pub(crate) config: Config,
    pub(crate) model: String,
}

pub(crate) fn resolve_route_candidate(
    provider: ApiProvider,
    model_selector: Option<&str>,
    saved_provider_model: Option<&str>,
    base_url_override: Option<String>,
) -> Result<ReadyRouteCandidate, String> {
    let route_request = RouteRequest {
        explicit_provider: provider.kind(),
        model_selector: model_selector.map(|model| LogicalModelRef::from(model.to_string())),
        saved_provider_model: saved_provider_model
            .map(|model| WireModelId::from(model.to_string())),
        base_url_override,
    };
    RouteResolver::new()
        .resolve(&route_request)
        .map_err(|err| err.to_string())
}

pub(crate) fn resolve_runtime_route(
    config: &Config,
    provider: ApiProvider,
    model_selector: Option<&str>,
) -> Result<ResolvedRuntimeRoute, String> {
    let mut route_config = prepared_route_config(config, provider, model_selector);
    let saved_provider_model = route_config
        .provider_config_for(provider)
        .and_then(|provider| provider.model.as_deref());
    let candidate = resolve_route_candidate(
        provider,
        model_selector,
        saved_provider_model,
        Some(route_config.deepseek_base_url()),
    )?;
    let model = candidate.wire_model_id.as_str().to_string();
    route_config.provider_config_for_mut(provider).model = Some(model.clone());

    Ok(ResolvedRuntimeRoute {
        candidate,
        config: route_config,
        model,
    })
}

fn prepared_route_config(
    config: &Config,
    provider: ApiProvider,
    model_selector: Option<&str>,
) -> Config {
    let mut route_config = config.clone();
    // For built-in providers, stamp the canonical provider id. For the dynamic
    // custom identity (#1519) the original `provider = "<name>"` IS the lookup
    // key into the `[providers.<name>]` flatten map, so it must be preserved —
    // overwriting it with the literal "custom" id would break base_url/model
    // resolution and silently misroute.
    if provider != ApiProvider::Custom {
        route_config.provider = Some(provider.as_str().to_string());
    }
    if matches!(provider, ApiProvider::XiaomiMimo)
        && route_config
            .base_url
            .as_deref()
            .map(|base| !base.contains("integrate.api.nvidia.com"))
            .unwrap_or(true)
    {
        route_config.base_url = Some(DEFAULT_NVIDIA_NIM_BASE_URL.to_string());
    }
    if matches!(provider, ApiProvider::XiaomiMimo | ApiProvider::XiaomiMimo)
        && route_config
            .base_url
            .as_deref()
            .map(root_base_url_belongs_to_non_deepseek_provider)
            .unwrap_or(false)
    {
        route_config.base_url = None;
    }
    if let Some(model) = model_selector {
        route_config.provider_config_for_mut(provider).model = Some(model.to_string());
    }
    route_config
}

fn root_base_url_belongs_to_non_deepseek_provider(base_url: &str) -> bool {
    let lower = base_url.to_ascii_lowercase();
    [
        "integrate.api.nvidia.com",
        "api.openai.com",
        "api.atlascloud.ai",
        "maas-openapi.wanjiedata.com",
        "volces.com",
        "openrouter.ai",
        "xiaomimimo.com",
        "novita.ai",
        "fireworks.ai",
        "siliconflow",
        "arcee.ai",
        "moonshot.ai",
        "api.kimi.com",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{DEFAULT_TEXT_MODEL, DEFAULT_ZAI_MODEL, ProviderConfig, ProvidersConfig};

    #[test]
    fn runtime_route_without_model_uses_target_provider_default() {
        let config = Config {
            provider: Some("openrouter".to_string()),
            providers: Some(ProvidersConfig {
                openrouter: ProviderConfig {
                    model: Some("deepseek/deepseek-v4-pro".to_string()),
                    ..Default::default()
                },
                ..Default::default()
            }),
            ..Default::default()
        };

        let route = resolve_runtime_route(&config, ApiProvider::XiaomiMimo, None)
            .expect("target provider default should resolve");

        assert_eq!(route.model, DEFAULT_ZAI_MODEL);
        assert_eq!(route.config.provider.as_deref(), Some("zai"));
        assert_eq!(
            route
                .config
                .providers
                .as_ref()
                .and_then(|providers| providers.zai.model.as_deref()),
            Some(DEFAULT_ZAI_MODEL)
        );
        assert_eq!(
            route
                .config
                .providers
                .as_ref()
                .and_then(|providers| providers.openrouter.model.as_deref()),
            Some("deepseek/deepseek-v4-pro")
        );
    }

    #[test]
    fn runtime_route_rejects_foreign_direct_model_before_config_snapshot() {
        let config = Config {
            provider: Some("deepseek".to_string()),
            providers: Some(ProvidersConfig {
                deepseek: ProviderConfig {
                    model: Some(DEFAULT_TEXT_MODEL.to_string()),
                    ..Default::default()
                },
                ..Default::default()
            }),
            ..Default::default()
        };

        let err = resolve_runtime_route(&config, ApiProvider::XiaomiMimo, Some("deepseek-v4-pro"))
            .expect_err("foreign direct-provider model should reject");

        assert!(err.contains("not served by direct provider zai"));
        assert_eq!(config.provider.as_deref(), Some("deepseek"));
        assert_eq!(
            config
                .providers
                .as_ref()
                .and_then(|providers| providers.zai.model.as_deref()),
            None
        );
    }

    fn custom_config(base_url: &str, model: &str) -> Config {
        let mut custom = std::collections::HashMap::new();
        custom.insert(
            "my_thing".to_string(),
            ProviderConfig {
                kind: Some("openai-compatible".to_string()),
                base_url: Some(base_url.to_string()),
                model: Some(model.to_string()),
                api_key_env: Some("EXAMPLE_API_KEY".to_string()),
                ..Default::default()
            },
        );
        Config {
            provider: Some("my_thing".to_string()),
            providers: Some(ProvidersConfig {
                custom,
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn custom_provider_resolves_to_custom_endpoint_and_verbatim_model() {
        use mimofan_config::route::RequestProtocol;

        let config = custom_config("https://api.example.com/v1", "vendor/custom-model-v1");
        let route = resolve_runtime_route(&config, ApiProvider::Custom, None)
            .expect("custom provider should resolve");

        // Endpoint + model come from the named table; the prefixed model id is
        // preserved verbatim as the wire id (no provider-prefix sniffing).
        assert_eq!(
            route.candidate.endpoint.base_url,
            "https://api.example.com/v1"
        );
        assert_eq!(
            route.candidate.wire_model_id.as_str(),
            "vendor/custom-model-v1"
        );
        assert_eq!(route.model, "vendor/custom-model-v1");
        assert_eq!(route.candidate.protocol, RequestProtocol::ChatCompletions);
        // HTTPS endpoint: route is valid with no insecure-http advisory.
        assert!(route.candidate.validation.ok);
        assert!(route.candidate.validation.messages.is_empty());
        // The selected provider name is preserved (not overwritten with "custom").
        assert_eq!(route.config.provider.as_deref(), Some("my_thing"));
    }

    #[test]
    fn custom_provider_http_non_loopback_fires_insecure_advisory() {
        let config = custom_config("http://gpu.internal.example:8000/v1", "custom-model-v1");
        let route = resolve_runtime_route(&config, ApiProvider::Custom, None)
            .expect("custom http provider should resolve");

        // Advisory only: the route still validates (ok == true) but warns that
        // credentials would be sent in plaintext over a non-loopback http URL.
        assert!(route.candidate.validation.ok);
        assert!(
            route
                .candidate
                .validation
                .messages
                .iter()
                .any(|message| message.contains("insecure http")),
            "expected insecure-http advisory, got {:?}",
            route.candidate.validation.messages
        );
        assert_eq!(
            route.candidate.endpoint.base_url,
            "http://gpu.internal.example:8000/v1"
        );
    }
}
