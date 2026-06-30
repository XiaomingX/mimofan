//! Fleet profile vocabulary, local profile discovery, and config-facing aliases.

#![allow(dead_code)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;

#[allow(unused_imports)]
pub use mimofan_config::{
    FleetDelegationHints, FleetLoadout, FleetProfile, FleetProfilePermissions, FleetRole, FleetSlot,
};

pub const WORKSPACE_AGENT_PROFILE_DIR: &str = ".mimofan/agents";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentProfile {
    pub id: String,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub profile: FleetProfile,
    pub source: PathBuf,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AgentProfileToml {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    role_hint: Option<String>,
    #[serde(default)]
    base_role: Option<String>,
    #[serde(default)]
    persona: Option<String>,
    #[serde(default)]
    model_class_hint: Option<String>,
    #[serde(default)]
    route_tier: Option<String>,
    #[serde(default)]
    loadout: Option<String>,
    #[serde(default, alias = "model_hint", alias = "model_id")]
    model: Option<String>,
    #[serde(default)]
    instructions: Option<AgentProfileInstructions>,
    #[serde(default)]
    tools: Option<AgentProfileTools>,
    #[serde(default)]
    permissions: Option<AgentProfilePermissionsToml>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AgentProfileInstructions {
    #[serde(default)]
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AgentProfileTools {
    #[serde(default)]
    posture: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AgentProfilePermissionsToml {
    #[serde(default)]
    allow_shell: Option<bool>,
    #[serde(default)]
    trust: Option<bool>,
    #[serde(default)]
    approval_required: Option<bool>,
}

pub fn load_workspace_agent_profiles(workspace: impl AsRef<Path>) -> Result<Vec<AgentProfile>> {
    load_agent_profiles_from_dir(workspace.as_ref().join(WORKSPACE_AGENT_PROFILE_DIR))
}

pub fn load_agent_profiles_from_dir(dir: impl AsRef<Path>) -> Result<Vec<AgentProfile>> {
    let dir = dir.as_ref();
    if !dir.exists() {
        return Ok(Vec::new());
    }
    if !dir.is_dir() {
        bail!("agent profile path {} is not a directory", dir.display());
    }

    let mut entries = std::fs::read_dir(dir)
        .with_context(|| format!("reading agent profile dir {}", dir.display()))?
        .collect::<std::io::Result<Vec<_>>>()
        .with_context(|| format!("reading agent profile entries in {}", dir.display()))?;
    entries.sort_by_key(|entry| entry.path());

    let mut profiles = Vec::new();
    let mut seen = BTreeSet::new();
    for entry in entries {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("toml") {
            continue;
        }
        let profile = load_agent_profile_file(&path)?;
        if !seen.insert(profile.id.clone()) {
            bail!("duplicate agent profile id {}", profile.id);
        }
        profiles.push(profile);
    }
    Ok(profiles)
}

fn load_agent_profile_file(path: &Path) -> Result<AgentProfile> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("reading agent profile {}", path.display()))?;
    let parsed: AgentProfileToml = toml::from_str(&raw)
        .map_err(|err| anyhow!("parsing agent profile {}: {err}", path.display()))?;
    agent_profile_from_toml(path, parsed)
}

fn agent_profile_from_toml(path: &Path, parsed: AgentProfileToml) -> Result<AgentProfile> {
    reject_permission_expansion(path, parsed.tools.as_ref(), parsed.permissions.as_ref())?;

    let fallback_id = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("profile");
    let id = first_present([parsed.id.as_deref(), parsed.name.as_deref()])
        .unwrap_or(fallback_id)
        .to_string();
    validate_agent_profile_token(path, "id/name", &id)?;

    let role_name = first_present([
        parsed.base_role.as_deref(),
        parsed.role_hint.as_deref(),
        parsed.name.as_deref(),
    ])
    .unwrap_or(&id)
    .to_string();
    validate_agent_profile_token(path, "base_role/role_hint", &role_name)?;

    let loadout = first_present([
        parsed.model_class_hint.as_deref(),
        parsed.route_tier.as_deref(),
        parsed.loadout.as_deref(),
    ])
    .map(FleetLoadout::from_name)
    .unwrap_or_default();
    let model = non_empty_trimmed(parsed.model.as_deref()).map(str::to_string);
    validate_agent_profile_model_hint(path, model.as_deref())?;

    let instructions = parsed
        .instructions
        .as_ref()
        .and_then(|instructions| non_empty_trimmed(instructions.text.as_deref()))
        .or_else(|| non_empty_trimmed(parsed.persona.as_deref()))
        .map(str::to_string);

    let description = non_empty_trimmed(parsed.description.as_deref()).map(str::to_string);
    let profile = FleetProfile {
        slot: FleetSlot::from_name(&role_name),
        role: FleetRole {
            name: role_name,
            description: description.clone(),
            instructions,
        },
        loadout,
        model,
        permissions: FleetProfilePermissions::default(),
        delegation: FleetDelegationHints::default(),
    };

    Ok(AgentProfile {
        id,
        display_name: non_empty_trimmed(parsed.display_name.as_deref()).map(str::to_string),
        description,
        profile,
        source: path.to_path_buf(),
    })
}

fn reject_permission_expansion(
    path: &Path,
    tools: Option<&AgentProfileTools>,
    permissions: Option<&AgentProfilePermissionsToml>,
) -> Result<()> {
    if let Some(posture) = tools
        .and_then(|tools| tools.posture.as_deref())
        .and_then(trimmed_non_empty)
    {
        match posture {
            "read-only" | "readonly" | "read_only" => {}
            other => bail!(
                "agent profile {} tools.posture={other:?} would widen permissions; use FleetProfile policy for grants",
                path.display()
            ),
        }
    }

    if let Some(permissions) = permissions {
        if permissions.allow_shell.unwrap_or(false) {
            bail!(
                "agent profile {} may not request allow_shell=true",
                path.display()
            );
        }
        if permissions.trust.unwrap_or(false) {
            bail!(
                "agent profile {} may not request trust=true",
                path.display()
            );
        }
        if permissions.approval_required == Some(false) {
            bail!(
                "agent profile {} may not disable approval_required",
                path.display()
            );
        }
    }
    Ok(())
}

fn validate_agent_profile_token(path: &Path, field: &str, value: &str) -> Result<()> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        bail!("agent profile {} {field} cannot be empty", path.display());
    }
    if trimmed != value || !trimmed.chars().all(is_agent_profile_token_char) {
        bail!(
            "agent profile {} {field} must be a simple token",
            path.display()
        );
    }
    Ok(())
}

fn validate_agent_profile_model_hint(path: &Path, value: Option<&str>) -> Result<()> {
    let Some(value) = value else {
        return Ok(());
    };
    if !is_model_hint(value) {
        bail!(
            "agent profile {} model must be a visible model id without whitespace or secrets",
            path.display()
        );
    }
    Ok(())
}

fn is_agent_profile_token_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.')
}

fn is_model_hint(value: &str) -> bool {
    let trimmed = value.trim();
    !trimmed.is_empty()
        && trimmed == value
        && trimmed
            .chars()
            .all(|ch| ch.is_ascii_graphic() && !matches!(ch, '=' | '\'' | '"'))
}

fn first_present<'a>(values: impl IntoIterator<Item = Option<&'a str>>) -> Option<&'a str> {
    values.into_iter().flatten().find_map(trimmed_non_empty)
}

fn non_empty_trimmed(value: Option<&str>) -> Option<&str> {
    value.and_then(trimmed_non_empty)
}

fn trimmed_non_empty(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

#[cfg(test)]
mod tests {}
