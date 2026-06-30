//! Execpolicy rules loaded from TOML configuration.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

use super::matcher::pattern_matches;
use crate::command_safety::prefix_allow_matches;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecPolicyDecision {
    Allow,
    Deny(String),
    AskUser(String),
}

#[derive(Debug, Deserialize, Default)]
pub struct ExecPolicyConfig {
    #[serde(default)]
    pub rules: BTreeMap<String, RuleSet>,
}

#[derive(Debug, Deserialize, Default)]
pub struct RuleSet {
    #[serde(default)]
    pub allow: Vec<String>,
    #[serde(default)]
    pub deny: Vec<String>,
}

impl ExecPolicyConfig {
    pub fn from_str(contents: &str) -> Result<Self> {
        toml::from_str(contents).context("failed to parse execpolicy.toml")
    }

    pub fn from_path(path: &Path) -> Result<Self> {
        let contents = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read execpolicy file {}", path.display()))?;
        Self::from_str(&contents)
    }

    pub fn evaluate(&self, command: &str) -> ExecPolicyDecision {
        for (group, rules) in &self.rules {
            for pattern in &rules.deny {
                if pattern_matches(pattern, command) {
                    return ExecPolicyDecision::Deny(format!(
                        "execpolicy denied by {group}: {pattern}"
                    ));
                }
            }
        }

        for (group, rules) in &self.rules {
            for pattern in &rules.allow {
                // Allow rules use arity-aware prefix matching first so that
                // `allow = ["git status"]` matches `git status -s` but NOT
                // `git push origin main`.  Fall back to regex-style
                // `pattern_matches` for wildcard patterns (e.g. `cargo *`).
                if prefix_allow_matches(pattern, command) || pattern_matches(pattern, command) {
                    let _ = group;
                    return ExecPolicyDecision::Allow;
                }
            }
        }

        ExecPolicyDecision::AskUser("execpolicy: no matching allow rule".to_string())
    }
}

pub fn default_execpolicy_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".deepseek").join("execpolicy.toml"))
}

pub fn load_default_policy() -> Result<Option<ExecPolicyConfig>> {
    let Some(path) = default_execpolicy_path() else {
        return Ok(None);
    };
    if !path.exists() {
        return Ok(None);
    }
    ExecPolicyConfig::from_path(&path).map(Some)
}

#[cfg(test)]
mod tests {}
