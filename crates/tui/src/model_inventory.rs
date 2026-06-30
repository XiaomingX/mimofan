//! Provider/model inventory for routing policy.
//!
//! This is the high-level "what can this user actually run?" object. Auto
//! routing, fleet workers, and sub-agent policy should consume this shape
//! instead of guessing model strings from global defaults.

use serde::Serialize;

use crate::config::{
    ApiProvider, Config, has_api_key_for, model_completion_names_for_provider,
    normalize_model_name_for_provider, provider_capability,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ModelAuthSource {
    Config,
    Command,
    Env,
    OAuthCli,
    Secret,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ModelRouteCandidate {
    pub(crate) provider: ApiProvider,
    pub(crate) provider_name: &'static str,
    pub(crate) provider_display_name: &'static str,
    pub(crate) model: String,
    pub(crate) context_window: u32,
    pub(crate) max_output: u32,
    pub(crate) thinking_supported: bool,
    pub(crate) cache_telemetry_supported: bool,
    pub(crate) auth_source: ModelAuthSource,
    pub(crate) default_for_provider: bool,
    pub(crate) tags: Vec<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ModelInventory {
    pub(crate) active_provider: ApiProvider,
    pub(crate) router_provider: ApiProvider,
    pub(crate) router_model: &'static str,
    pub(crate) router_available: bool,
    pub(crate) candidates: Vec<ModelRouteCandidate>,
}

impl ModelInventory {
    pub(crate) fn from_config(config: &Config) -> Self {
        let active_provider = config.api_provider();
        let mut candidates = Vec::new();

        for provider in ApiProvider::all().iter().copied() {
            let Some(auth_source) = auth_source_for_provider(config, provider) else {
                continue;
            };

            let default_model = provider_default_model(config, provider);
            let mut models = Vec::<String>::new();
            if let Some(model) = configured_model_for_provider(config, provider) {
                push_model(&mut models, provider, &model);
            }
            if provider == active_provider {
                let active_model = config.default_model();
                if !active_model.trim().eq_ignore_ascii_case("auto") {
                    push_model(&mut models, provider, &active_model);
                }
            }
            for model in model_completion_names_for_provider(provider) {
                push_model(&mut models, provider, model);
            }
            if models.is_empty() {
                push_model(&mut models, provider, &default_model);
            }

            for model in models {
                let capability = provider_capability(provider, &model);
                let mut tags = Vec::new();
                if capability.context_window >= 1_000_000 {
                    tags.push("long_context");
                }
                if capability.thinking_supported {
                    tags.push("thinking");
                }
                if model.eq_ignore_ascii_case(&default_model) {
                    tags.push("default");
                }

                candidates.push(ModelRouteCandidate {
                    provider,
                    provider_name: provider.as_str(),
                    provider_display_name: provider.display_name(),
                    default_for_provider: model.eq_ignore_ascii_case(&default_model),
                    model,
                    context_window: capability.context_window,
                    max_output: capability.max_output,
                    thinking_supported: capability.thinking_supported,
                    cache_telemetry_supported: capability.cache_telemetry_supported,
                    auth_source: auth_source.clone(),
                    tags,
                });
            }
        }

        Self {
            active_provider,
            router_provider: ApiProvider::XiaomiMimo,
            router_model: "deepseek-v4-flash",
            router_available: has_api_key_for(config, ApiProvider::XiaomiMimo),
            candidates,
        }
    }

    pub(crate) fn candidate(
        &self,
        provider: ApiProvider,
        model: &str,
    ) -> Option<&ModelRouteCandidate> {
        self.candidates.iter().find(|candidate| {
            candidate.provider == provider && candidate.model.eq_ignore_ascii_case(model.trim())
        })
    }

    pub(crate) fn active_default(&self) -> Option<&ModelRouteCandidate> {
        self.candidates
            .iter()
            .find(|candidate| {
                candidate.provider == self.active_provider && candidate.default_for_provider
            })
            .or_else(|| {
                self.candidates
                    .iter()
                    .find(|candidate| candidate.provider == self.active_provider)
            })
            .or_else(|| self.candidates.first())
    }

    pub(crate) fn router_context_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }
}

fn push_model(models: &mut Vec<String>, provider: ApiProvider, model: &str) {
    let Some(model) = normalize_model_name_for_provider(provider, model)
        .or_else(|| crate::config::normalize_custom_model_id(model))
    else {
        return;
    };
    if !models
        .iter()
        .any(|existing| existing.eq_ignore_ascii_case(&model))
    {
        models.push(model);
    }
}

fn configured_model_for_provider(config: &Config, provider: ApiProvider) -> Option<String> {
    config
        .provider_config_for(provider)
        .and_then(|entry| entry.model.clone())
        .map(|model| model.trim().to_string())
        .filter(|model| !model.is_empty())
}

fn provider_default_model(config: &Config, provider: ApiProvider) -> String {
    if provider == config.api_provider() {
        let model = config.default_model();
        if !model.trim().eq_ignore_ascii_case("auto") {
            return model;
        }
    }
    model_completion_names_for_provider(provider)
        .first()
        .copied()
        .unwrap_or(match provider {
            _ => crate::config::DEFAULT_TEXT_MODEL,
        })
        .to_string()
}

fn auth_source_for_provider(config: &Config, provider: ApiProvider) -> Option<ModelAuthSource> {
    if env_has_key_for(provider) {
        return Some(ModelAuthSource::Env);
    }
    if let Some(auth) = config
        .provider_config_for(provider)
        .and_then(|entry| entry.auth.as_ref())
    {
        return match auth.source {
            mimofan_config::AuthSourceKind::Command => Some(ModelAuthSource::Command),
            mimofan_config::AuthSourceKind::Secret => Some(ModelAuthSource::Secret),
        };
    }
    if provider_uses_oauth_cli(config, provider) && has_api_key_for(config, provider) {
        return Some(ModelAuthSource::OAuthCli);
    }
    has_api_key_for(config, provider).then_some(ModelAuthSource::Config)
}

fn provider_uses_oauth_cli(config: &Config, provider: ApiProvider) -> bool {
    match provider {
        ApiProvider::XiaomiMimo => true,
        ApiProvider::XiaomiMimo => config
            .provider_config_for(provider)
            .and_then(|entry| entry.auth_mode.as_deref())
            .is_some_and(|mode| {
                let mode = mode.trim().to_ascii_lowercase().replace('-', "_");
                matches!(mode.as_str(), "kimi" | "kimi_oauth" | "kimi_cli" | "oauth")
            }),
        _ => false,
    }
}

fn env_has_key_for(provider: ApiProvider) -> bool {
    env_keys_for_provider(provider)
        .iter()
        .any(|key| std::env::var(key).is_ok_and(|value| !value.trim().is_empty()))
}

fn env_keys_for_provider(provider: ApiProvider) -> &'static [&'static str] {
    provider.env_vars()
}

#[cfg(test)]
mod tests {}
