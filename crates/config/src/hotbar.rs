//! Hotbar slot configuration types and resolution.
//!
//! Extracted from `lib.rs` during the config crate split
//! (CODE_STRUCTURE_ANALYSIS.md §3.3).
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use serde::{Deserialize, Serialize};

pub const HOTBAR_SLOT_COUNT: u8 = 8;

pub const DEFAULT_HOTBAR_ACTIONS: [&str; HOTBAR_SLOT_COUNT as usize] = [
    "voice.toggle",
    "session.compact",
    "mode.plan",
    "mode.agent",
    "mode.yolo",
    "palette.open",
    "sidebar.toggle",
    "trust.toggle",
];

/// On-disk schema for one `[[hotbar]]` table.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct HotbarBindingToml {
    pub slot: u8,
    pub action: String,
    #[serde(default)]
    pub label: Option<String>,
}

/// Validated hotbar binding used by future render/dispatch layers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotbarBinding {
    pub slot: u8,
    pub action: String,
    pub label: Option<String>,
}

/// Non-fatal hotbar config issue. Invalid slots are skipped; duplicate slots
/// use the last binding; unknown actions are kept for UI feedback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotbarConfigWarning {
    SlotOutOfRange {
        slot: u8,
        action: String,
    },
    DuplicateSlot {
        slot: u8,
        previous_action: String,
        replacement_action: String,
    },
    UnknownAction {
        slot: u8,
        action: String,
    },
}

impl fmt::Display for HotbarConfigWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SlotOutOfRange { slot, action } => write!(
                f,
                "hotbar slot {slot} for action '{action}' is outside 1-{HOTBAR_SLOT_COUNT}; skipped"
            ),
            Self::DuplicateSlot {
                slot,
                previous_action,
                replacement_action,
            } => write!(
                f,
                "hotbar slot {slot} was bound to '{previous_action}' more than once; using '{replacement_action}'"
            ),
            Self::UnknownAction { slot, action } => write!(
                f,
                "hotbar slot {slot} references unknown action '{action}'; keeping binding"
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotbarConfigResolution {
    pub bindings: Vec<HotbarBinding>,
    pub warnings: Vec<HotbarConfigWarning>,
}

#[must_use]
pub fn default_hotbar_bindings() -> Vec<HotbarBinding> {
    DEFAULT_HOTBAR_ACTIONS
        .iter()
        .enumerate()
        .map(|(idx, action)| HotbarBinding {
            slot: u8::try_from(idx + 1).expect("default hotbar slot fits in u8"),
            action: (*action).to_string(),
            label: None,
        })
        .collect()
}

#[must_use]
pub fn resolve_hotbar_bindings(
    configured: Option<&[HotbarBindingToml]>,
    known_action_ids: &[&str],
) -> HotbarConfigResolution {
    let known = known_action_ids.iter().copied().collect::<BTreeSet<&str>>();
    let mut warnings = Vec::new();

    let source = match configured {
        Some(bindings) => bindings
            .iter()
            .map(|binding| HotbarBinding {
                slot: binding.slot,
                action: binding.action.clone(),
                label: binding.label.clone(),
            })
            .collect::<Vec<_>>(),
        None => default_hotbar_bindings(),
    };

    let mut by_slot: BTreeMap<u8, HotbarBinding> = BTreeMap::new();
    for binding in source {
        if !(1..=HOTBAR_SLOT_COUNT).contains(&binding.slot) {
            warnings.push(HotbarConfigWarning::SlotOutOfRange {
                slot: binding.slot,
                action: binding.action,
            });
            continue;
        }
        if !known.is_empty() && !known.contains(binding.action.as_str()) {
            warnings.push(HotbarConfigWarning::UnknownAction {
                slot: binding.slot,
                action: binding.action.clone(),
            });
        }
        if let Some(previous) = by_slot.insert(binding.slot, binding.clone()) {
            warnings.push(HotbarConfigWarning::DuplicateSlot {
                slot: binding.slot,
                previous_action: previous.action,
                replacement_action: binding.action,
            });
        }
    }

    HotbarConfigResolution {
        bindings: by_slot.into_values().collect(),
        warnings,
    }
}

