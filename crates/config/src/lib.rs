pub mod auth_source;
pub mod catalog;
mod fleet;
mod harness;
pub mod models_dev;
pub mod pricing;
pub mod provider;
mod provider_defaults;
mod provider_kind;
pub mod route;
pub use fleet::{
    DEFAULT_SPAWN_DEPTH, FleetConfigToml, FleetDelegationHints, FleetExecConfig, FleetLoadout,
    FleetProfile, FleetProfilePermissions, FleetRole, FleetRolePreset, FleetSlot,
    MAX_SPAWN_DEPTH_CEILING, built_in_role_presets,
};
pub use harness::{
    HarnessCompactionStrategy, HarnessPosture, HarnessPostureKind, HarnessProfile,
    HarnessSafetyPosture, HarnessToolSurface, built_in_harness_profiles,
};
pub use provider_defaults::*;
pub use provider_kind::ProviderKind;

// Split from the original `lib.rs` godfile (CODE_STRUCTURE_ANALYSIS.md §3.3).
pub mod hotbar;
pub mod permissions;
pub mod provider_config;
pub mod surface_config;
pub use hotbar::*;
pub use permissions::*;
pub use provider_config::*;
pub use surface_config::*;
// Bring the per-provider config get/set helpers (pub(crate)) into scope so the
// root `impl ConfigToml` can keep calling them after the split.
use crate::provider_config::{
    get_provider_config_display_value, get_provider_config_value, insert_provider_config_values,
    parse_provider_config_key, set_provider_config_value, unset_provider_config_value,
};

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fs;
#[cfg(unix)]
use std::io::Read;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{Context, Result, bail};
pub use auth_source::{AuthSourceKind, ProviderAuthSourceToml};
use mimofan_execpolicy::ExecPolicyEngine;
pub use mimofan_execpolicy::ToolAskRule;
use mimofan_secrets::SecretSource;
pub use mimofan_secrets::Secrets;
use serde::{Deserialize, Serialize};

#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

pub const CONFIG_FILE_NAME: &str = "config.toml";
pub const PERMISSIONS_FILE_NAME: &str = "permissions.toml";

/// Sibling `permissions.toml` schema.
///
/// Each rule is a typed condition that means "ask before this tool invocation."
/// Typed allow/deny records and UI actions are expected to land in follow-up PRs.

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ConfigToml {
    /// TUI-compatible DeepSeek API key. Kept at the root so both `deepseek`
    /// and `mimofan` can share a single config file.
    pub api_key: Option<String>,
    /// TUI-compatible DeepSeek base URL.
    pub base_url: Option<String>,
    /// Optional extra HTTP headers forwarded to model API requests.
    #[serde(default)]
    pub http_headers: BTreeMap<String, String>,
    /// TUI-compatible default DeepSeek model.
    pub default_text_model: Option<String>,
    #[serde(default)]
    pub provider: ProviderKind,
    pub model: Option<String>,
    pub auth_mode: Option<String>,
    pub output_mode: Option<String>,
    pub verbosity: Option<String>,
    pub log_level: Option<String>,
    pub telemetry: Option<bool>,
    pub approval_policy: Option<String>,
    pub sandbox_mode: Option<String>,
    /// Native tool catalog controls shared with `mimofan`.
    #[serde(default)]
    pub tools: Option<ToolsToml>,
    #[serde(default)]
    pub providers: ProvidersToml,
    /// Provider fallback chain (#2574). TUI runtime code may advance through
    /// these providers after recoverable provider errors; config resolution
    /// itself still reports the selected primary provider.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fallback_providers: Vec<ProviderKind>,
    /// Per-domain network policy (#135). When absent, network tools fall back
    /// to a permissive default that mirrors pre-v0.7.0 behavior.
    #[serde(default)]
    pub network: Option<NetworkPolicyToml>,
    /// Community skill installer settings (#140). Mirrors
    /// [`SkillsToml`] from the TUI side; the dispatcher consults
    /// `registry_url` when running `deepseek skill install`.
    #[serde(default)]
    pub skills: Option<SkillsToml>,
    /// Workspace side-git snapshots (#137). The live TUI defaults this to
    /// enabled with 7-day retention when absent.
    #[serde(default)]
    pub snapshots: Option<SnapshotsToml>,
    /// Post-edit LSP diagnostics injection (#136). When absent, the engine
    /// applies the defaults documented in [`LspConfigToml`].
    #[serde(default)]
    pub lsp: Option<LspConfigToml>,
    /// Per-model harness profiles (#2693). This is the durable config data model.
    #[serde(default)]
    pub harness_profiles: Vec<HarnessProfile>,
    /// Optional 1-8 hotbar slot bindings (#2064). When absent, the TUI falls
    /// back to the built-in default slots.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hotbar: Option<Vec<HotbarBindingToml>>,
    /// App-server hook sink configuration. Kept separate from the TUI
    /// lifecycle `[hooks]` table so config rewrites preserve existing hooks.
    #[serde(default)]
    pub hook_sinks: Option<HookSinksToml>,
    /// Agent Fleet trust and security policy (#3165). When absent, fleet
    /// workers inherit conservative Sandbox defaults.
    #[serde(default)]
    pub fleet: Option<FleetConfigToml>,
    #[serde(flatten)]
    pub extras: BTreeMap<String, toml::Value>,
}

impl ConfigToml {
    /// Merge safe project-level overrides from `$WORKSPACE/.mimofan/config.toml`.
    ///
    /// Repo-local config is untrusted input. This helper intentionally ignores
    /// credentials, endpoints, provider selection, auth/session values, telemetry,
    /// network policy, skill registry, LSP command tables, and unknown extras.
    /// Approval and sandbox values may only tighten the existing user/global
    /// posture.
    pub fn merge_project_overrides(&mut self, project: ConfigToml) {
        if project.default_text_model.is_some() {
            self.default_text_model = project.default_text_model;
        }
        if project.model.is_some() {
            self.model = project.model;
        }
        if project.output_mode.is_some() {
            self.output_mode = project.output_mode;
        }
        if project.verbosity.is_some() {
            self.verbosity = project.verbosity;
        }
        if project.log_level.is_some() {
            self.log_level = project.log_level;
        }
        if let Some(policy) = project.approval_policy
            && project_approval_policy_is_allowed(self.approval_policy.as_deref(), &policy)
        {
            self.approval_policy = Some(policy);
        }
        if let Some(mode) = project.sandbox_mode
            && project_sandbox_mode_is_allowed(self.sandbox_mode.as_deref(), &mode)
        {
            self.sandbox_mode = Some(mode);
        }
        if project.tools.is_some() {
            self.tools = project.tools;
        }
        for provider in ProviderKind::ALL {
            merge_project_provider_config(
                self.providers.for_provider_mut(provider),
                project.providers.for_provider(provider),
            );
        }
    }

    #[must_use]
    pub fn get_value(&self, key: &str) -> Option<String> {
        if let Some((provider, field)) = parse_provider_config_key(key) {
            return get_provider_config_value(self.providers.for_provider(provider), field);
        }

        match key {
            "provider" => Some(self.provider.as_str().to_string()),
            "api_key" => self.api_key.clone(),
            "base_url" => self.base_url.clone(),
            "http_headers" => serialize_http_headers(&self.http_headers),
            "default_text_model" => self.default_text_model.clone(),
            "model" => self.model.clone(),
            "auth.mode" => self.auth_mode.clone(),
            "output_mode" => self.output_mode.clone(),
            "verbosity" => self.verbosity.clone(),
            "log_level" => self.log_level.clone(),
            "telemetry" => self.telemetry.map(|v| v.to_string()),
            "approval_policy" => self.approval_policy.clone(),
            "sandbox_mode" => self.sandbox_mode.clone(),
            "tools.always_load" => self.tools.as_ref().map(|tools| tools.always_load.join(",")),
            "hook_sinks.unix_socket_path" => self
                .hook_sinks
                .as_ref()
                .and_then(|sinks| sinks.unix_socket_path.as_ref())
                .map(|path| path.display().to_string()),
            _ => self.extras.get(key).map(toml::Value::to_string),
        }
    }

    #[must_use]
    pub fn get_display_value(&self, key: &str) -> Option<String> {
        if let Some((provider, field)) = parse_provider_config_key(key) {
            return get_provider_config_display_value(self.providers.for_provider(provider), field);
        }

        if key == "http_headers" {
            return serialize_http_headers_for_display(&self.http_headers);
        }

        if let Some(value) = self.extras.get(key) {
            return Some(redact_toml_value_for_display(key, value));
        }

        self.get_value(key).map(|value| {
            if is_sensitive_config_key(key) {
                redact_secret(&value)
            } else {
                value
            }
        })
    }

    pub fn set_value(&mut self, key: &str, value: &str) -> Result<()> {
        if let Some((provider, field)) = parse_provider_config_key(key) {
            return set_provider_config_value(self, provider, field, value);
        }

        match key {
            "provider" => {
                self.provider = ProviderKind::parse(value).with_context(|| {
                    format!(
                        "unknown provider '{value}': expected {}",
                        ProviderKind::names_hint()
                    )
                })?;
            }
            "api_key" => self.api_key = Some(value.to_string()),
            "base_url" => self.base_url = Some(value.to_string()),
            "http_headers" => self.http_headers = parse_http_headers(value)?,
            "default_text_model" => self.default_text_model = Some(value.to_string()),
            "model" => self.model = Some(value.to_string()),
            "auth.mode" => self.auth_mode = Some(value.to_string()),
            "output_mode" => self.output_mode = Some(value.to_string()),
            "verbosity" => self.verbosity = Some(value.to_string()),
            "log_level" => self.log_level = Some(value.to_string()),
            "telemetry" => {
                self.telemetry = Some(parse_bool(value)?);
            }
            "approval_policy" => self.approval_policy = Some(value.to_string()),
            "sandbox_mode" => self.sandbox_mode = Some(value.to_string()),
            "hook_sinks.unix_socket_path" => {
                self.hook_sinks
                    .get_or_insert_with(HookSinksToml::default)
                    .unix_socket_path = Some(PathBuf::from(value));
            }
            _ => {
                self.extras
                    .insert(key.to_string(), toml::Value::String(value.to_string()));
            }
        }
        Ok(())
    }

    pub fn unset_value(&mut self, key: &str) -> Result<()> {
        if let Some((provider, field)) = parse_provider_config_key(key) {
            unset_provider_config_value(self, provider, field);
            return Ok(());
        }

        match key {
            "provider" => self.provider = ProviderKind::OpenAiCompatible,
            "api_key" => self.api_key = None,
            "base_url" => self.base_url = None,
            "http_headers" => self.http_headers.clear(),
            "default_text_model" => self.default_text_model = None,
            "model" => self.model = None,
            "auth.mode" => self.auth_mode = None,
            "output_mode" => self.output_mode = None,
            "verbosity" => self.verbosity = None,
            "log_level" => self.log_level = None,
            "telemetry" => self.telemetry = None,
            "approval_policy" => self.approval_policy = None,
            "sandbox_mode" => self.sandbox_mode = None,
            "hook_sinks.unix_socket_path" => {
                if let Some(sinks) = self.hook_sinks.as_mut() {
                    sinks.unix_socket_path = None;
                }
            }
            _ => {
                self.extras.remove(key);
            }
        }
        Ok(())
    }

    #[must_use]
    pub fn list_values(&self) -> BTreeMap<String, String> {
        let mut out = BTreeMap::new();
        out.insert("provider".to_string(), self.provider.as_str().to_string());

        if let Some(v) = self.api_key.as_ref() {
            out.insert("api_key".to_string(), redact_secret(v));
        }
        if let Some(v) = self.base_url.as_ref() {
            out.insert("base_url".to_string(), v.clone());
        }
        if let Some(v) = serialize_http_headers_for_display(&self.http_headers) {
            out.insert("http_headers".to_string(), v);
        }
        if let Some(v) = self.default_text_model.as_ref() {
            out.insert("default_text_model".to_string(), v.clone());
        }
        if let Some(v) = self.model.as_ref() {
            out.insert("model".to_string(), v.clone());
        }
        if let Some(v) = self.auth_mode.as_ref() {
            out.insert("auth.mode".to_string(), v.clone());
        }
        if let Some(v) = self.output_mode.as_ref() {
            out.insert("output_mode".to_string(), v.clone());
        }
        if let Some(v) = self.verbosity.as_ref() {
            out.insert("verbosity".to_string(), v.clone());
        }
        if let Some(v) = self.log_level.as_ref() {
            out.insert("log_level".to_string(), v.clone());
        }
        if let Some(v) = self.telemetry {
            out.insert("telemetry".to_string(), v.to_string());
        }
        if let Some(v) = self.approval_policy.as_ref() {
            out.insert("approval_policy".to_string(), v.clone());
        }
        if let Some(v) = self.sandbox_mode.as_ref() {
            out.insert("sandbox_mode".to_string(), v.clone());
        }
        if let Some(v) = self
            .hook_sinks
            .as_ref()
            .and_then(|sinks| sinks.unix_socket_path.as_ref())
        {
            out.insert(
                "hook_sinks.unix_socket_path".to_string(),
                v.display().to_string(),
            );
        }

        for provider in ProviderKind::ALL {
            insert_provider_config_values(
                &mut out,
                provider,
                self.providers.for_provider(provider),
            );
        }

        for (k, v) in &self.extras {
            out.insert(k.clone(), redact_toml_value_for_display(k, v));
        }
        out
    }

    /// Resolve runtime options without touching platform credential stores.
    ///
    /// This method keeps library callers prompt-free: CLI flag → config file
    /// → environment. Call `resolve_runtime_options_with_secrets` when a
    /// user-facing dispatcher should recover credentials from the configured
    /// secret store.
    #[must_use]
    pub fn resolve_runtime_options(&self, cli: &CliRuntimeOverrides) -> ResolvedRuntimeOptions {
        let no_keyring = Secrets::new(std::sync::Arc::new(
            mimofan_secrets::InMemoryKeyringStore::new(),
        ));
        self.resolve_runtime_options_with_secrets(cli, &no_keyring)
    }

    /// Resolve runtime options using an explicit secrets façade.
    ///
    /// API-key precedence is **CLI flag → config-file → secret store → environment**.
    #[must_use]
    pub fn resolve_runtime_options_with_secrets(
        &self,
        cli: &CliRuntimeOverrides,
        secrets: &Secrets,
    ) -> ResolvedRuntimeOptions {
        let env = EnvRuntimeOverrides::load();
        let (provider, provider_source) = if let Some(provider) = cli.provider {
            (provider, ProviderSource::Cli)
        } else if let Some(provider) = env.provider {
            (
                provider,
                ProviderSource::Env(env.provider_source.unwrap_or("MIMOFAN_PROVIDER")),
            )
        } else if env.custom_base_url.is_some() {
            (ProviderKind::OpenAiCompatible, ProviderSource::Env("CUSTOM_BASE_URL"))
        } else {
            (self.provider, ProviderSource::Config)
        };

        let provider_cfg = self.providers.for_provider(provider).clone();
        let auth_mode = cli
            .auth_mode
            .clone()
            .or_else(|| env.auth_mode.clone())
            .or_else(|| provider_cfg.auth_mode.clone())
            .or_else(|| self.auth_mode.clone());
        let from_file = provider_cfg.api_key.clone().or(self.api_key.clone());
        let configured_base_url = cli
            .base_url
            .clone()
            .or_else(|| env.base_url_for(provider))
            .or_else(|| provider_cfg.base_url.clone())
            .or(self.base_url.clone());
        let env_api_key = env_api_key_for_provider(provider);
        let base_url = configured_base_url
            .unwrap_or_else(|| default_base_url_for_provider(provider).to_string());
        // API-key precedence is **CLI flag → environment → config-file → secret store**.
        let (api_key, api_key_source) = if let Some(value) = cli.api_key.clone() {
            (Some(value), Some(RuntimeApiKeySource::Cli))
        } else if let Some(value) = env_api_key.filter(|v| !v.trim().is_empty()) {
            (Some(value), Some(RuntimeApiKeySource::Env))
        } else if let Some(value) = from_file.filter(|v| !v.trim().is_empty()) {
            (Some(value), Some(RuntimeApiKeySource::ConfigFile))
        } else if should_skip_secret_store_for_provider(provider, &base_url, auth_mode.as_deref()) {
            (None, None)
        } else {
            match secrets.resolve_with_source(provider.as_str()) {
                Some((value, source)) => {
                    let source = match source {
                        SecretSource::Keyring => RuntimeApiKeySource::Keyring,
                        SecretSource::Env => RuntimeApiKeySource::Env,
                    };
                    (Some(value), Some(source))
                }
                None => (None, None),
            }
        };

        let env_provider_model = env.model_for(provider, &base_url);
        let explicit_model = cli.model.is_some()
            || env.model.is_some()
            || env_provider_model.is_some()
            || provider_cfg.model.is_some()
            || self.default_text_model.is_some()
            || self.model.is_some();
        let model = cli
            .model
            .clone()
            .or_else(|| env.model.clone())
            .or(env_provider_model)
            .or_else(|| provider_cfg.model.clone())
            .or(self.default_text_model.clone())
            .or_else(|| self.model.clone())
            .unwrap_or_else(|| default_model_for_provider(provider).to_string());
        let model =
            if explicit_model && provider_preserves_custom_base_url_model(provider, &base_url) {
                model.trim().to_string()
            } else {
                normalize_model_for_provider(provider, &model)
            };

        let mut http_headers = self.http_headers.clone();
        http_headers.extend(provider_cfg.http_headers.clone());
        if let Some(env_headers) = env.http_headers {
            http_headers.extend(env_headers);
        }
        http_headers.retain(|name, value| !name.trim().is_empty() && !value.trim().is_empty());

        let output_mode = cli
            .output_mode
            .clone()
            .or_else(|| env.output_mode.clone())
            .or_else(|| self.output_mode.clone());
        let log_level = cli
            .log_level
            .clone()
            .or_else(|| env.log_level.clone())
            .or_else(|| self.log_level.clone());
        let telemetry = cli
            .telemetry
            .or(env.telemetry)
            .or(self.telemetry)
            .unwrap_or(false);
        let approval_policy = cli
            .approval_policy
            .clone()
            .or_else(|| env.approval_policy.clone())
            .or_else(|| self.approval_policy.clone());
        let sandbox_mode = cli
            .sandbox_mode
            .clone()
            .or_else(|| env.sandbox_mode.clone())
            .or_else(|| self.sandbox_mode.clone());
        let yolo = cli.yolo.or(env.yolo);
        let verbosity = cli
            .verbosity
            .clone()
            .or_else(|| env.verbosity.clone())
            .or_else(|| self.verbosity.clone());

        ResolvedRuntimeOptions {
            provider,
            provider_source,
            model,
            api_key,
            api_key_source,
            base_url,
            auth_mode,
            insecure_skip_tls_verify: provider_cfg.insecure_skip_tls_verify.unwrap_or(false),
            output_mode,
            log_level,
            telemetry,
            approval_policy,
            sandbox_mode,
            yolo,
            verbosity,
            http_headers,
        }
    }
}

fn merge_project_provider_config(target: &mut ProviderConfigToml, source: &ProviderConfigToml) {
    if source.model.is_some() {
        target.model = source.model.clone();
    }
}

#[must_use]
pub fn project_approval_policy_is_allowed(current: Option<&str>, project: &str) -> bool {
    let Some(project_rank) = approval_policy_rank(project) else {
        return false;
    };
    match current.and_then(approval_policy_rank) {
        Some(current_rank) => project_rank >= current_rank,
        None => project_rank >= 2,
    }
}

#[must_use]
pub fn project_sandbox_mode_is_allowed(current: Option<&str>, project: &str) -> bool {
    let normalized_project = project.trim().to_ascii_lowercase();
    if normalized_project == "external-sandbox" {
        return current
            .map(|value| value.trim().eq_ignore_ascii_case("external-sandbox"))
            .unwrap_or(false);
    }

    let Some(project_rank) = sandbox_mode_rank(project) else {
        return false;
    };
    match current.and_then(sandbox_mode_rank) {
        Some(current_rank) => project_rank >= current_rank,
        None => project_rank >= 2,
    }
}

fn approval_policy_rank(value: &str) -> Option<u8> {
    match value.trim().to_ascii_lowercase().as_str() {
        "auto" => Some(0),
        "suggest" | "suggested" | "on-request" | "untrusted" => Some(1),
        "never" | "deny" | "denied" => Some(2),
        _ => None,
    }
}

fn sandbox_mode_rank(value: &str) -> Option<u8> {
    match value.trim().to_ascii_lowercase().as_str() {
        "danger-full-access" => Some(0),
        "external-sandbox" => Some(0),
        "workspace-write" => Some(1),
        "read-only" => Some(2),
        _ => None,
    }
}

/// Load a project-level config from `$WORKSPACE/.mimofan/config.toml`.
/// Returns `None` if the file doesn't exist or can't be parsed.
pub fn load_project_config(workspace: &Path) -> Option<ConfigToml> {
    let path = workspace.join(MIMOFAN_APP_DIR).join(CONFIG_FILE_NAME);
    if !project_config_candidate_exists(&path) {
        return None;
    }
    let raw = match read_checked_config_file(&path) {
        Ok(raw) => raw,
        Err(e) => {
            tracing::warn!("Failed to read project config {}: {e:#}", path.display());
            return None;
        }
    };
    match toml::from_str(&raw) {
        Ok(config) => Some(config),
        Err(e) => {
            tracing::warn!("Failed to parse project config {}: {e}", path.display());
            None
        }
    }
}

fn project_config_candidate_exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| {
        let file_type = metadata.file_type();
        file_type.is_file() || file_type.is_symlink()
    })
}

fn normalize_model_for_provider(_provider: ProviderKind, model: &str) -> String {
    // 模型名原样透传：具体别名归一化由对应网关负责，mimofan 不再绑定产品专属模型。
    model.to_string()
}

/// Canonicalize compact DeepSeek model aliases to stable IDs.
///
/// Single source of truth shared by the tui and cli crates. Already-valid
/// model IDs pass through unchanged; only the compact `v4pro`/`v4flash`
/// spellings are rewritten to their hyphenated forms. Returns `None` when the
/// input is not a recognised DeepSeek alias (callers decide fallthrough).
#[must_use]
pub fn canonical_model_name(model: &str) -> Option<&'static str> {
    match model.trim().to_ascii_lowercase().as_str() {
        "pro" | "deepseek-v4pro" => Some("deepseek-v4-pro"),
        "flash" | "deepseek-v4flash" => Some("deepseek-v4-flash"),
        _ => None,
    }
}

fn default_model_for_provider(provider: ProviderKind) -> &'static str {
    provider.provider().default_model()
}

pub fn default_base_url_for_provider(provider: ProviderKind) -> &'static str {
    provider.provider().default_base_url()
}

fn base_url_is_custom_for_provider(provider: ProviderKind, base_url: &str) -> bool {
    let actual = base_url.trim_end_matches('/');
    let default = default_base_url_for_provider(provider).trim_end_matches('/');
    actual != default
}

fn provider_preserves_custom_base_url_model(provider: ProviderKind, base_url: &str) -> bool {
    base_url_is_custom_for_provider(provider, base_url)
}

fn should_skip_secret_store_for_provider(
    _provider: ProviderKind,
    base_url: &str,
    auth_mode: Option<&str>,
) -> bool {
    if auth_mode_requires_api_key(auth_mode) {
        return false;
    }
    if auth_mode_disables_api_key(auth_mode) {
        return true;
    }

    base_url_uses_local_host(base_url)
}

fn env_api_key_for_provider(provider: ProviderKind) -> Option<String> {
    mimofan_secrets::env_for(provider.as_str())
}

fn auth_mode_requires_api_key(auth_mode: Option<&str>) -> bool {
    matches!(
        auth_mode
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.to_ascii_lowercase()),
        Some(value)
            if matches!(
                value.as_str(),
                "api_key" | "api-key" | "apikey" | "bearer" | "bearer-token"
            )
    )
}

fn auth_mode_disables_api_key(auth_mode: Option<&str>) -> bool {
    matches!(
        auth_mode
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.to_ascii_lowercase()),
        Some(value)
            if matches!(
                value.as_str(),
                "none" | "off" | "disabled" | "no_auth" | "no-auth" | "anonymous"
            )
    )
}

fn base_url_uses_local_host(base_url: &str) -> bool {
    let Some(host) = base_url_host(base_url) else {
        return false;
    };
    let host = host.trim_matches(['[', ']']).to_ascii_lowercase();
    if matches!(host.as_str(), "localhost" | "0.0.0.0") {
        return true;
    }
    host.parse::<std::net::IpAddr>()
        .is_ok_and(|addr| addr.is_loopback() || addr.is_unspecified())
}

fn base_url_host(base_url: &str) -> Option<&str> {
    let without_scheme = base_url
        .split_once("://")
        .map_or(base_url, |(_, rest)| rest);
    let authority = without_scheme.split('/').next()?.rsplit('@').next()?;
    if let Some(rest) = authority.strip_prefix('[') {
        return rest.split_once(']').map(|(host, _)| host);
    }
    authority.split(':').next().filter(|host| !host.is_empty())
}

#[derive(Debug, Clone, Default)]
pub struct CliRuntimeOverrides {
    pub provider: Option<ProviderKind>,
    pub model: Option<String>,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub auth_mode: Option<String>,
    pub output_mode: Option<String>,
    pub log_level: Option<String>,
    pub telemetry: Option<bool>,
    pub approval_policy: Option<String>,
    pub sandbox_mode: Option<String>,
    pub yolo: Option<bool>,
    pub verbosity: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeApiKeySource {
    Cli,
    ConfigFile,
    Keyring,
    Env,
}

impl RuntimeApiKeySource {
    #[must_use]
    pub fn as_env_value(self) -> &'static str {
        match self {
            Self::Cli => "cli",
            Self::ConfigFile => "config",
            Self::Keyring => "keyring",
            Self::Env => "env",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderSource {
    Cli,
    Env(&'static str),
    Config,
}

#[derive(Debug, Clone)]
pub struct ResolvedRuntimeOptions {
    pub provider: ProviderKind,
    pub provider_source: ProviderSource,
    pub model: String,
    pub api_key: Option<String>,
    pub api_key_source: Option<RuntimeApiKeySource>,
    pub base_url: String,
    pub auth_mode: Option<String>,
    pub insecure_skip_tls_verify: bool,
    pub output_mode: Option<String>,
    pub log_level: Option<String>,
    pub telemetry: bool,
    pub approval_policy: Option<String>,
    pub sandbox_mode: Option<String>,
    pub yolo: Option<bool>,
    pub verbosity: Option<String>,
    pub http_headers: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct ConfigStore {
    path: PathBuf,
    pub config: ConfigToml,
    permissions: PermissionsToml,
    /// Original file text, retained so [`save`](Self::save) can merge
    /// comments back after serialisation.
    original_raw: Option<String>,
}

impl ConfigStore {
    pub fn load(path: Option<PathBuf>) -> Result<Self> {
        let path = resolve_config_path(path)?;
        let (config, original_raw) = if checked_path_exists(&path)? {
            let raw = read_checked_config_file(&path)?;
            let parsed: ConfigToml = toml::from_str(&raw)
                .with_context(|| format!("failed to parse config at {}", path.display()))?;
            (parsed, Some(raw))
        } else {
            (ConfigToml::default(), None)
        };
        let permissions = load_sibling_permissions(&path)?;

        Ok(Self {
            path,
            config,
            permissions,
            original_raw,
        })
    }

    pub fn save(&self) -> Result<()> {
        let path = normalize_config_file_path(self.path.clone())?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create config directory {}", parent.display())
            })?;
        }
        let body = if let Some(ref original_raw) = self.original_raw {
            let serialized =
                toml::to_string_pretty(&self.config).context("failed to serialize config")?;
            merge_and_preserve_comments(&serialized, original_raw).unwrap_or_else(|e| {
                tracing::warn!("failed to merge config comments, saving without them: {e:#}");
                serialized
            })
        } else {
            toml::to_string_pretty(&self.config).context("failed to serialize config")?
        };
        if checked_path_exists(&path)? {
            let existing = read_checked_config_file(&path)?;
            if existing == body {
                return Ok(());
            }
            write_one_time_config_backup(&path)?;
        }
        #[cfg(unix)]
        {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&path)
                .with_context(|| format!("failed to write config at {}", path.display()))?;
            file.write_all(body.as_bytes())
                .with_context(|| format!("failed to write config at {}", path.display()))?;
            file.set_permissions(fs::Permissions::from_mode(0o600))
                .with_context(|| {
                    format!("failed to set config permissions at {}", path.display())
                })?;
        }
        #[cfg(not(unix))]
        {
            fs::write(&path, body)
                .with_context(|| format!("failed to write config at {}", path.display()))?;
        }
        Ok(())
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    #[must_use]
    pub fn permissions(&self) -> &PermissionsToml {
        &self.permissions
    }

    #[must_use]
    pub fn permissions_path(&self) -> PathBuf {
        checked_permissions_path_for_config_path(&self.path)
            .expect("ConfigStore path is validated before construction")
    }

    #[must_use]
    pub fn exec_policy_engine(&self) -> ExecPolicyEngine {
        if self.permissions.is_empty() {
            ExecPolicyEngine::new(Vec::new(), Vec::new())
        } else {
            ExecPolicyEngine::with_rulesets(vec![self.permissions.ruleset()])
        }
    }

    /// Atomically append ask-only permission rules to the sibling
    /// `permissions.toml` file.
    ///
    /// Existing comments and formatting are preserved. Exact duplicate rules
    /// are ignored, and the in-memory permissions snapshot is refreshed after
    /// a successful write.
    pub fn append_ask_rules(&mut self, rules: &[ToolAskRule]) -> Result<usize> {
        if rules.is_empty() {
            return Ok(0);
        }

        let path = checked_permissions_path_for_config_path(&self.path)?;
        let raw = if checked_path_exists(&path)? {
            read_checked_permissions_file(&path)?
        } else {
            String::new()
        };
        let mut permissions = if raw.trim().is_empty() {
            PermissionsToml::default()
        } else {
            toml::from_str(&raw)
                .with_context(|| format!("failed to parse permissions at {}", path.display()))?
        };
        let mut document = if raw.trim().is_empty() {
            toml_edit::DocumentMut::new()
        } else {
            raw.parse::<toml_edit::DocumentMut>()
                .with_context(|| format!("failed to edit permissions at {}", path.display()))?
        };

        if !document.contains_key("rules") {
            document["rules"] = toml_edit::Item::ArrayOfTables(toml_edit::ArrayOfTables::new());
        }
        let rules_item = document
            .get_mut("rules")
            .expect("rules entry was inserted above");

        let mut added = 0;
        for rule in rules {
            if permissions.rules.contains(rule) {
                continue;
            }
            append_ask_rule(rules_item, rule)?;
            permissions.rules.push(rule.clone());
            added += 1;
        }
        if added == 0 {
            self.permissions = permissions;
            return Ok(0);
        }

        let body = document.to_string();
        let persisted: PermissionsToml = toml::from_str(&body).with_context(|| {
            format!(
                "generated invalid permissions document for {}",
                path.display()
            )
        })?;
        write_permissions_atomic(&path, body.as_bytes())?;
        self.permissions = persisted;
        Ok(added)
    }
}

fn config_backup_file_name(path: &Path) -> OsString {
    let mut file_name = path
        .file_name()
        .map(OsString::from)
        .unwrap_or_else(|| OsString::from(CONFIG_FILE_NAME));
    file_name.push(".bak");
    file_name
}

fn config_sibling_path_unchecked(config_path: &Path, file_name: &OsStr) -> PathBuf {
    config_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(file_name)
}

fn checked_config_sibling_path(config_path: &Path, file_name: &OsStr) -> Result<PathBuf> {
    let config_path = normalize_config_file_path(config_path.to_path_buf())?;
    let parent = config_path
        .parent()
        .context("config path must include a parent directory")?;
    let path = parent.join(file_name);
    reject_path_symlink(&path)?;
    Ok(path)
}

fn checked_config_backup_path(path: &Path) -> Result<PathBuf> {
    checked_config_sibling_path(path, &config_backup_file_name(path))
}

fn write_one_time_config_backup(path: &Path) -> Result<()> {
    let backup = checked_config_backup_path(path)?;
    if backup.exists() {
        return Ok(());
    }
    fs::copy(path, &backup).with_context(|| {
        format!(
            "failed to create config backup {} from {}",
            backup.display(),
            path.display()
        )
    })?;
    #[cfg(unix)]
    {
        fs::set_permissions(&backup, fs::Permissions::from_mode(0o600)).with_context(|| {
            format!(
                "failed to set config backup permissions at {}",
                backup.display()
            )
        })?;
    }
    Ok(())
}

/// Merge comments and formatting from an original TOML file into a
/// freshly serialized document so user annotations (comments, whitespace,
/// disabled keys) survive config rewrites.
///
/// `original_raw` is the raw text of the file before the change; the
/// function parses it internally with [`toml_edit`] so callers stay free
/// of that dependency.
pub fn merge_and_preserve_comments(serialized: &str, original_raw: &str) -> Result<String> {
    let original = original_raw
        .parse::<toml_edit::DocumentMut>()
        .context("failed to parse original config for comment merge")?;

    let mut new_doc = serialized
        .parse::<toml_edit::DocumentMut>()
        .context("failed to parse serialized config for comment merge")?;

    // Reuse the original document’s trailing text (file-footer comments /
    // disabled keys) so they survive the rewrite.
    new_doc.set_trailing(original.trailing().clone());

    // Copy the top-level table's decor (document-header comments, whitespace
    // before the first key) which `toml_edit` stores on the root `Table` itself.
    *new_doc.as_table_mut().decor_mut() = original.as_table().decor().clone();

    merge_decor_table(new_doc.as_table_mut(), original.as_table());

    Ok(new_doc.to_string())
}

/// Recursively copy `decor` (prefix/suffix comments and whitespace) from
/// every key in `source` that also exists in `target`.
fn merge_decor_table(target: &mut toml_edit::Table, source: &toml_edit::Table) {
    // Collect keys first — the borrow checker won't let us hold
    // `get_key_value_mut` while iterating.
    let keys: Vec<String> = source.iter().map(|(k, _)| k.to_owned()).collect();
    for key in &keys {
        let Some((source_key, source_item)) = source.get_key_value(key) else {
            continue;
        };
        let Some((mut target_key_mut, target_item)) = target.get_key_value_mut(key) else {
            continue;
        };

        // Copy the key-level decor (comments before the key itself)
        *target_key_mut.leaf_decor_mut() = source_key.leaf_decor().clone();

        copy_item_decor(target_item, source_item);

        if let (Some(tt), Some(st)) = (target_item.as_table_mut(), source_item.as_table()) {
            merge_decor_table(tt, st);
        }

        if let (Some(ta), Some(sa)) = (
            target_item.as_array_of_tables_mut(),
            source_item.as_array_of_tables(),
        ) {
            for (i, source_table) in sa.iter().enumerate() {
                if let Some(target_table) = ta.get_mut(i) {
                    copy_item_decor_table(target_table, source_table);
                    merge_decor_table(target_table, source_table);
                }
            }
        }
    }
}

/// Copy the decor (comments and surrounding whitespace) from `source` to `target`,
/// respecting the concrete item type since [`toml_edit::Item`] has no uniform
/// `decor` accessor.
fn copy_item_decor(target: &mut toml_edit::Item, source: &toml_edit::Item) {
    match (target, source) {
        (toml_edit::Item::Table(tt), toml_edit::Item::Table(st)) => {
            *tt.decor_mut() = st.decor().clone();
        }
        (toml_edit::Item::Value(tv), toml_edit::Item::Value(sv)) => {
            *tv.decor_mut() = sv.decor().clone();
        }
        _ => {}
    }
}

fn copy_item_decor_table(target: &mut toml_edit::Table, source: &toml_edit::Table) {
    *target.decor_mut() = source.decor().clone();
}

/// Process-wide default [`Secrets`] façade. The first caller wins; the
/// lock is exposed so test or CLI code can install an explicit
/// backend (e.g. an [`mimofan_secrets::InMemoryKeyringStore`]) before
/// any resolver runs.
pub fn default_secrets() -> &'static Secrets {
    static SECRETS: OnceLock<Secrets> = OnceLock::new();
    SECRETS.get_or_init(|| {
        // Tests should never poke real platform credential stores. Cargo sets the
        // `RUST_TEST_*` family of env vars (and `CARGO_PKG_NAME` is
        // always populated), but the `cfg(test)` flag is the canonical
        // signal here. See `install_test_secrets` for explicit installs.
        #[cfg(test)]
        {
            Secrets::new(std::sync::Arc::new(
                mimofan_secrets::InMemoryKeyringStore::new(),
            ))
        }
        #[cfg(not(test))]
        {
            Secrets::auto_detect()
        }
    })
}

// ── mimofan state root (v0.8.44) ──────────────────────────────────
//
/// Canonical mimofan app directory name under $HOME.
pub const MIMOFAN_APP_DIR: &str = ".mimofan";

/// Resolve the primary mimofan home directory.
///
/// `$MIMO_HOME` (or `$MIMOFAN_HOME` as fallback) takes precedence when set.
/// Otherwise defaults to `$HOME/.mimo`. This is the write target for new product state.
pub fn mimofan_home() -> Result<PathBuf> {
    if let Some(path) = mimofan_home_env_override() {
        return Ok(path);
    }
    let home = effective_home_dir().context("failed to resolve home directory")?;
    Ok(home.join(MIMOFAN_APP_DIR))
}

fn mimofan_home_env_override() -> Option<PathBuf> {
    let val = std::env::var("MIMOFAN_HOME").ok()?;
    let trimmed = val.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(PathBuf::from(trimmed))
    }
}

fn effective_home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(dirs::home_dir)
}

/// Reject state subdirs that could escape the state root via path injection.
///
/// `ensure_state_dir` / `resolve_state_dir` are public APIs taking an arbitrary
/// subdir string; every in-tree caller passes a hardcoded single component
/// (e.g. `"sessions"`, `"."`). This validates defensively so a future caller
/// can never traverse out of the state root via `..` components or an absolute
/// path. Nested relative paths such as `"a/b"` are permitted.
fn ensure_safe_state_subdir(subdir: &str) -> Result<()> {
    if subdir.is_empty() {
        bail!("state subdir must not be empty");
    }
    let path = std::path::Path::new(subdir);
    if path.is_absolute() {
        bail!("state subdir must not be an absolute path: {subdir}");
    }
    if path.components().any(|c| {
        matches!(
            c,
            std::path::Component::RootDir | std::path::Component::Prefix(_)
        )
    }) {
        bail!("state subdir must not contain a root or prefix: {subdir}");
    }
    if path
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        bail!("state subdir must not contain parent-dir (..) components: {subdir}");
    }
    Ok(())
}

/// Resolve a state subdirectory under the mimofan home (`~/.mimofan`).
pub fn resolve_state_dir(subdir: &str) -> Result<PathBuf> {
    ensure_safe_state_subdir(subdir)?;
    Ok(mimofan_home()?.join(subdir))
}

/// Ensure a state subdirectory exists under the primary mimofan root,
/// creating it if necessary. This is the write-path resolver.
///
/// On the first creation of a real subdirectory (not the root sentinel `"."`),
/// Ensure a state subdirectory exists under `~/.mimofan/`,
/// creating it if necessary. Returns the directory path.
pub fn ensure_state_dir(subdir: &str) -> Result<PathBuf> {
    ensure_safe_state_subdir(subdir)?;
    let dir = mimofan_home()?.join(subdir);
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("failed to create {}/", dir.display()))?;
    Ok(dir)
}

/// Resolve a project-local state subdirectory under `.mimofan/`.
pub fn resolve_project_state_dir(workspace: &Path, subdir: &str) -> Result<PathBuf> {
    ensure_safe_state_subdir(subdir)?;
    let workspace = normalize_project_workspace(workspace)?;
    Ok(workspace.join(MIMOFAN_APP_DIR).join(subdir))
}

/// Ensure a project-local state subdirectory exists under `.mimofan/`,
/// creating it if necessary. Returns the directory path.
pub fn ensure_project_state_dir(workspace: &Path, subdir: &str) -> Result<PathBuf> {
    ensure_safe_state_subdir(subdir)?;
    let workspace = normalize_project_workspace(workspace)?;
    let dir = workspace.join(MIMOFAN_APP_DIR).join(subdir);
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("failed to create {}/", dir.display()))?;
    Ok(dir)
}

pub fn resolve_config_path(explicit: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(path) = explicit {
        return normalize_config_file_path(path);
    }
    if let Ok(path) = std::env::var("MIMOFAN_CONFIG_PATH") {
        if let Some(path) = config_path_from_env_value(&path)? {
            return Ok(path);
        }
        return default_config_path();
    }
    default_config_path()
}

fn config_path_from_env_value(path: &str) -> Result<Option<PathBuf>> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        Ok(None)
    } else {
        normalize_config_file_path(PathBuf::from(trimmed)).map(Some)
    }
}

#[must_use]
pub fn permissions_path_for_config_path(config_path: &Path) -> PathBuf {
    config_sibling_path_unchecked(config_path, OsStr::new(PERMISSIONS_FILE_NAME))
}

fn checked_permissions_path_for_config_path(config_path: &Path) -> Result<PathBuf> {
    checked_config_sibling_path(config_path, OsStr::new(PERMISSIONS_FILE_NAME))
}

pub fn resolve_permissions_path(config_path: Option<PathBuf>) -> Result<PathBuf> {
    checked_permissions_path_for_config_path(&resolve_config_path(config_path)?)
}

fn load_sibling_permissions(config_path: &Path) -> Result<PermissionsToml> {
    let permissions_path = checked_permissions_path_for_config_path(config_path)?;
    if !checked_path_exists(&permissions_path)? {
        return Ok(PermissionsToml::default());
    }

    let raw = read_checked_permissions_file(&permissions_path)?;
    toml::from_str(&raw).with_context(|| {
        format!(
            "failed to parse permissions at {}",
            permissions_path.display()
        )
    })
}

fn append_ask_rule(item: &mut toml_edit::Item, rule: &ToolAskRule) -> Result<()> {
    match item {
        toml_edit::Item::ArrayOfTables(rules) => {
            rules.push(ask_rule_table(rule));
            Ok(())
        }
        toml_edit::Item::Value(value) => {
            let Some(rules) = value.as_array_mut() else {
                bail!("`rules` in permissions.toml must be an array");
            };
            rules.push(toml_edit::Value::InlineTable(ask_rule_inline_table(rule)));
            Ok(())
        }
        _ => bail!("`rules` in permissions.toml must be an array"),
    }
}

fn ask_rule_table(rule: &ToolAskRule) -> toml_edit::Table {
    let mut table = toml_edit::Table::new();
    table["tool"] = toml_edit::value(rule.tool.clone());
    if let Some(command) = rule.command.as_deref() {
        table["command"] = toml_edit::value(command);
    }
    if let Some(path) = rule.path.as_deref() {
        table["path"] = toml_edit::value(path);
    }
    table
}

fn ask_rule_inline_table(rule: &ToolAskRule) -> toml_edit::InlineTable {
    let mut table = toml_edit::InlineTable::new();
    table.insert("tool", toml_edit::Value::from(rule.tool.clone()));
    if let Some(command) = rule.command.as_deref() {
        table.insert("command", toml_edit::Value::from(command));
    }
    if let Some(path) = rule.path.as_deref() {
        table.insert("path", toml_edit::Value::from(path));
    }
    table
}

fn write_permissions_atomic(path: &Path, body: &[u8]) -> Result<()> {
    let parent = path.parent().with_context(|| {
        format!(
            "permissions path has no parent directory: {}",
            path.display()
        )
    })?;
    fs::create_dir_all(parent).with_context(|| {
        format!(
            "failed to create permissions directory {}",
            parent.display()
        )
    })?;

    let mut temporary = tempfile::NamedTempFile::new_in(parent).with_context(|| {
        format!(
            "failed to create temporary permissions file in {}",
            parent.display()
        )
    })?;
    #[cfg(unix)]
    temporary
        .as_file()
        .set_permissions(fs::Permissions::from_mode(0o600))
        .with_context(|| {
            format!(
                "failed to secure temporary permissions file for {}",
                path.display()
            )
        })?;
    temporary
        .write_all(body)
        .with_context(|| format!("failed to write permissions at {}", path.display()))?;
    temporary
        .as_file()
        .sync_all()
        .with_context(|| format!("failed to sync permissions at {}", path.display()))?;
    temporary
        .persist(path)
        .map_err(|error| error.error)
        .with_context(|| format!("failed to replace permissions at {}", path.display()))?;
    Ok(())
}

pub fn default_config_path() -> Result<PathBuf> {
    Ok(mimofan_home()?.join(CONFIG_FILE_NAME))
}

fn parse_bool(raw: &str) -> Result<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" | "enabled" => Ok(true),
        "0" | "false" | "no" | "off" | "disabled" => Ok(false),
        _ => bail!("invalid boolean '{raw}'"),
    }
}

fn parse_http_headers(raw: &str) -> Result<BTreeMap<String, String>> {
    let mut headers = BTreeMap::new();
    for pair in raw.trim().split(',') {
        let pair = pair.trim();
        if pair.is_empty() {
            continue;
        }
        let Some((name, value)) = pair.split_once('=') else {
            bail!("invalid header pair '{pair}', expected name=value");
        };
        let name = name.trim();
        let value = value.trim();
        if name.is_empty() {
            bail!("header name cannot be empty");
        }
        if value.is_empty() {
            continue;
        }
        headers.insert(name.to_string(), value.to_string());
    }
    Ok(headers)
}

fn serialize_http_headers(headers: &BTreeMap<String, String>) -> Option<String> {
    if headers.is_empty() {
        return None;
    }
    Some(
        headers
            .iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join(","),
    )
}

fn serialize_http_headers_for_display(headers: &BTreeMap<String, String>) -> Option<String> {
    if headers.is_empty() {
        return None;
    }
    Some(
        headers
            .iter()
            .map(|(name, value)| {
                let display_value = if is_sensitive_config_key(name) {
                    redact_secret(value)
                } else {
                    value.clone()
                };
                format!("{name}={display_value}")
            })
            .collect::<Vec<_>>()
            .join(","),
    )
}

fn redact_secret(secret: &str) -> String {
    let chars: Vec<char> = secret.chars().collect();
    if chars.len() <= 16 {
        return "********".to_string();
    }
    let prefix: String = chars.iter().take(4).collect();
    let suffix: String = chars
        .iter()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{prefix}***{suffix}")
}

#[must_use]
pub fn is_sensitive_config_key(key: &str) -> bool {
    let Some(segment) = key.rsplit('.').next() else {
        return false;
    };
    let normalized = segment
        .trim()
        .trim_matches('"')
        .replace('-', "_")
        .to_ascii_lowercase();

    matches!(
        normalized.as_str(),
        "api_key"
            | "apikey"
            | "api_keys"
            | "authorization"
            | "bearer"
            | "client_secret"
            | "credential"
            | "credentials"
            | "id_token"
            | "password"
            | "passwords"
            | "passwd"
            | "proxy_authorization"
            | "refresh_token"
            | "secret"
            | "secrets"
            | "token"
            | "tokens"
    ) || normalized.ends_with("_api_key")
        || normalized.ends_with("_authorization")
        || normalized.ends_with("_password")
        || normalized.ends_with("_secret")
        || normalized.ends_with("_token")
}

fn redact_toml_value_for_display(key: &str, value: &toml::Value) -> String {
    redact_toml_value_for_display_inner(key, false, value).to_string()
}

fn redact_toml_value_for_display_inner(
    key: &str,
    sensitive_ancestor: bool,
    value: &toml::Value,
) -> toml::Value {
    let sensitive = sensitive_ancestor || is_sensitive_config_key(key);
    match value {
        toml::Value::String(value) if sensitive => toml::Value::String(redact_secret(value)),
        toml::Value::Array(values) => toml::Value::Array(
            values
                .iter()
                .map(|value| redact_toml_value_for_display_inner(key, sensitive, value))
                .collect(),
        ),
        toml::Value::Table(table) => {
            let mut redacted = toml::map::Map::new();
            for (child_key, child_value) in table {
                let path = if key.is_empty() {
                    child_key.clone()
                } else {
                    format!("{key}.{child_key}")
                };
                redacted.insert(
                    child_key.clone(),
                    redact_toml_value_for_display_inner(&path, sensitive, child_value),
                );
            }
            toml::Value::Table(redacted)
        }
        _ if sensitive => toml::Value::String("********".to_string()),
        _ => value.clone(),
    }
}

fn normalize_config_file_path(path: PathBuf) -> Result<PathBuf> {
    if path.as_os_str().is_empty() {
        bail!("config path cannot be empty");
    }
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        bail!("config path cannot contain '..' components");
    }
    if path.file_name().is_none() {
        bail!("config path must include a file name");
    }
    let absolute = if path.is_absolute() {
        path
    } else {
        std::env::current_dir()
            .context("failed to resolve current directory for config path")?
            .join(path)
    };
    let file_name = absolute
        .file_name()
        .map(OsString::from)
        .context("config path must include a file name")?;
    let parent = absolute
        .parent()
        .context("config path must include a parent directory")?;
    let parent = match parent.canonicalize() {
        Ok(parent) => parent,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => parent.to_path_buf(),
        Err(err) => {
            return Err(err).with_context(|| {
                format!("failed to resolve config directory {}", parent.display())
            });
        }
    };
    let normalized = parent.join(file_name);
    reject_path_symlink(&normalized)?;
    Ok(normalized)
}

fn normalize_project_workspace(workspace: &Path) -> Result<PathBuf> {
    if workspace.as_os_str().is_empty() {
        bail!("project workspace path cannot be empty");
    }
    if workspace
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        bail!("project workspace path cannot contain '..' components");
    }
    let absolute = if workspace.is_absolute() {
        workspace.to_path_buf()
    } else {
        std::env::current_dir()
            .context("failed to resolve current directory for project workspace")?
            .join(workspace)
    };
    match absolute.canonicalize() {
        Ok(path) => Ok(path),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            Ok(normalize_path_components(&absolute))
        }
        Err(err) => Err(err).with_context(|| {
            format!(
                "failed to resolve project workspace {}",
                workspace.display()
            )
        }),
    }
}

fn normalize_path_components(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(part) => normalized.push(part),
        }
    }
    if normalized.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        normalized
    }
}

fn checked_path_exists(path: &Path) -> Result<bool> {
    let path = normalize_config_file_path(path.to_path_buf())?;
    path.try_exists()
        .with_context(|| format!("failed to inspect config path {}", path.display()))
}

fn read_checked_config_file(path: &Path) -> Result<String> {
    read_checked_toml_file(path, "config")
}

fn read_checked_permissions_file(path: &Path) -> Result<String> {
    read_checked_toml_file(path, "permissions")
}

fn read_checked_toml_file(path: &Path, label: &str) -> Result<String> {
    let path = normalize_config_file_path(path.to_path_buf())?;
    read_string_no_follow(&path)
        .with_context(|| format!("failed to read {label} at {}", path.display()))
}

#[cfg(unix)]
fn read_string_no_follow(path: &Path) -> std::io::Result<String> {
    let mut file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    let mut raw = String::new();
    file.read_to_string(&mut raw)?;
    Ok(raw)
}

#[cfg(not(unix))]
fn read_string_no_follow(path: &Path) -> std::io::Result<String> {
    fs::read_to_string(path)
}

fn reject_path_symlink(path: &Path) -> Result<()> {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return Ok(());
    };
    if metadata.file_type().is_symlink() {
        bail!("config path must not be a symlink: {}", path.display());
    }
    Ok(())
}

#[derive(Debug, Clone, Default)]
struct EnvRuntimeOverrides {
    provider: Option<ProviderKind>,
    provider_source: Option<&'static str>,
    model: Option<String>,
    openai_compatible_model: Option<String>,
    output_mode: Option<String>,
    auth_mode: Option<String>,
    log_level: Option<String>,
    telemetry: Option<bool>,
    approval_policy: Option<String>,
    sandbox_mode: Option<String>,
    yolo: Option<bool>,
    verbosity: Option<String>,
    http_headers: Option<BTreeMap<String, String>>,
    openai_compatible_base_url: Option<String>,
    custom_base_url: Option<String>,
}

impl EnvRuntimeOverrides {
    fn load() -> Self {
        let (provider, provider_source) = Self::load_provider();
        Self {
            provider,
            provider_source,
            model: std::env::var("MIMOFAN_MODEL")
                .or_else(|_| std::env::var("MIMOFAN_MODEL"))
                .or_else(|_| std::env::var("MIMOFAN_DEFAULT_TEXT_MODEL"))
                .ok()
                .filter(|v| !v.trim().is_empty()),
            openai_compatible_model: std::env::var("XIAOMI_MIMO_MODEL")
                .ok()
                .filter(|v| !v.trim().is_empty()),
            verbosity: std::env::var("MIMOFAN_VERBOSITY")
                .or_else(|_| std::env::var("MIMOFAN_VERBOSITY"))
                .ok(),
            output_mode: std::env::var("MIMOFAN_OUTPUT_MODE").ok(),
            auth_mode: std::env::var("MIMOFAN_AUTH_MODE").ok(),
            log_level: std::env::var("MIMOFAN_LOG_LEVEL").ok(),
            telemetry: std::env::var("MIMOFAN_TELEMETRY")
                .ok()
                .and_then(|v| match parse_bool(&v) {
                    Ok(b) => Some(b),
                    Err(_) => {
                        tracing::warn!("Invalid MIMOFAN_TELEMETRY value '{v}', expected true/false");
                        None
                    }
                }),
            approval_policy: std::env::var("MIMOFAN_APPROVAL_POLICY").ok(),
            sandbox_mode: std::env::var("MIMOFAN_SANDBOX_MODE").ok(),
            yolo: std::env::var("MIMOFAN_YOLO")
                .ok()
                .and_then(|v| match parse_bool(&v) {
                    Ok(b) => Some(b),
                    Err(_) => {
                        tracing::warn!("Invalid MIMOFAN_YOLO value '{v}', expected true/false");
                        None
                    }
                }),
            http_headers: std::env::var("MIMOFAN_HTTP_HEADERS")
                .ok()
                .and_then(|value| match parse_http_headers(&value) {
                    Ok(h) => Some(h),
                    Err(_) => {
                        tracing::warn!("Invalid MIMOFAN_HTTP_HEADERS value, expected format: header1=val1,header2=val2");
                        None
                    }
                })
                .filter(|headers| !headers.is_empty()),
            openai_compatible_base_url: std::env::var("XIAOMI_MIMO_BASE_URL")
                .or_else(|_| std::env::var("ANTHROPIC_BASE_URL"))
                .ok()
                .filter(|v| !v.trim().is_empty()),
            custom_base_url: std::env::var("CUSTOM_BASE_URL")
                .or_else(|_| std::env::var("OPENAI_BASE_URL"))
                .or_else(|_| std::env::var("MIMOFAN_BASE_URL"))
                .ok()
                .filter(|v| !v.trim().is_empty()),
        }
    }

    fn load_provider() -> (Option<ProviderKind>, Option<&'static str>) {
        if let Ok(value) = std::env::var("MIMOFAN_PROVIDER") {
            let parsed = ProviderKind::parse(&value);
            return (parsed, parsed.map(|_| "MIMOFAN_PROVIDER"));
        }

        if std::env::var("CUSTOM_BASE_URL").is_ok()
            || std::env::var("OPENAI_BASE_URL").is_ok()
            || std::env::var("MIMOFAN_BASE_URL").is_ok()
        {
            return (Some(ProviderKind::OpenAiCompatible), Some("CUSTOM_BASE_URL"));
        }

        (None, None)
    }

    fn base_url_for(&self, provider: ProviderKind) -> Option<String> {
        // Defaults belong in the resolver's final fallback so config-file
        // values (`providers.<name>.base_url`) still win when env is unset.
        match provider {
            ProviderKind::OpenAiCompatible => self
                .custom_base_url
                .clone()
                .or_else(|| self.openai_compatible_base_url.clone()),
            ProviderKind::AnthropicCompatible => self.openai_compatible_base_url.clone(),
            ProviderKind::GeminiCompatible => self.openai_compatible_base_url.clone(),
        }
    }

    fn model_for(&self, provider: ProviderKind, base_url: &str) -> Option<String> {
        let model = match provider {
            ProviderKind::OpenAiCompatible => self.openai_compatible_model.clone(),
            _ => None,
        }?;

        if provider_preserves_custom_base_url_model(provider, base_url) {
            Some(model.trim().to_string())
        } else {
            Some(normalize_model_for_provider(provider, &model))
        }
    }
}
