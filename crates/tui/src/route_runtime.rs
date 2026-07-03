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
    if matches!(provider, ApiProvider::XiaomiMimo)
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
mod tests {}
