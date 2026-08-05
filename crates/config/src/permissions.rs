//! Sibling `permissions.toml` schema.
//!
//! Extracted from `lib.rs` during the config crate split
//! (CODE_STRUCTURE_ANALYSIS.md §3.3).
use serde::{Deserialize, Serialize};
use mimofan_execpolicy::{Ruleset, ToolAskRule};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PermissionsToml {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<ToolAskRule>,
}

impl PermissionsToml {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    #[must_use]
    pub fn ruleset(&self) -> Ruleset {
        Ruleset::user(Vec::new(), Vec::new()).with_ask_rules(self.rules.clone())
    }
}
