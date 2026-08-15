//! Plugin manifest definition and loading (issue #834, plan W1).
//!
//! The manifest describes which optional capabilities should be assembled into
//! the running agent. W1 only models the `tools.extra` list — a set of string
//! names selecting known extra tools (see `crate::plugins::registry`). Future
//! workstreams (W2/W4) extend the manifest with `sandbox`/`llm` capability
//! selectors.
//!
//! Serialization uses TOML, matching the rest of the crate's config loading
//! (`toml::from_str` is the established style in `config.rs`, etc.).

use std::path::Path;

use serde::{Deserialize, Serialize};

/// The `[tools]` table of a plugin manifest.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolsManifest {
    /// Names of known extra tools to register (e.g. `"hypothesis"`,
    /// `"run_poc"`, `"gadget_chain_trace"`). Unknown names are skipped by the
    /// assembler with a `tracing::warn!`.
    #[serde(default)]
    pub extra: Vec<String>,
}

/// Top-level plugin manifest.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginManifest {
    /// Optional tool capability selectors.
    #[serde(default)]
    pub tools: ToolsManifest,
}

impl PluginManifest {
    /// Load a manifest from a TOML file on disk.
    ///
    /// # Errors
    /// Returns an `std::io::Error` if the file cannot be read, or a
    /// `toml::de::Error` if it cannot be parsed as TOML.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, Box<dyn std::error::Error>> {
        let raw = std::fs::read_to_string(path)?;
        let manifest: PluginManifest = toml::from_str(&raw)?;
        Ok(manifest)
    }

    /// A manifest with no optional capabilities — equivalent to the legacy
    /// behavior where the tool registry is built purely from static `with_*`
    /// builders.
    #[must_use]
    pub fn from_defaults() -> Self {
        Self::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_have_empty_extra() {
        let manifest = PluginManifest::from_defaults();
        assert!(manifest.tools.extra.is_empty());
    }

    #[test]
    fn parses_extra_names_from_toml() {
        let toml_src = r#"
[tools]
extra = ["hypothesis", "run_poc"]
"#;
        let manifest: PluginManifest = toml::from_str(toml_src).expect("valid toml");
        assert_eq!(manifest.tools.extra, vec!["hypothesis", "run_poc"]);
    }
}
