//! Sub-agent naming utilities.
//!
//! Provides deterministic whale name assignment for sub-agents.

use std::collections::HashSet;
use std::hash::{Hash, Hasher};

/// MIMOFAN 鲸鱼昵称列表
const MIMOFAN_NICKNAMES: &[&str] = &[
    "Orca",
    "Beluga",
    "Narwhal",
    "Sperm Whale",
    "Blue Whale",
    "Humpback",
    "Fin Whale",
    "Gray Whale",
    "Right Whale",
    "Bowhead",
];

/// Return a deterministic whale name for a given agent ID using a hash of
/// the ID string. The same ID always gets the same name — stable across
/// session restarts for persisted agents.
#[must_use]
pub fn whale_name_for_id(id: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    id.hash(&mut hasher);
    let idx = (hasher.finish() as usize) % MIMOFAN_NICKNAMES.len();
    MIMOFAN_NICKNAMES[idx].to_string()
}

/// Assign a unique whale name for an agent ID, avoiding collisions with
/// names already in `active_names`. If the deterministic name is taken,
/// appends a numeric suffix (e.g. "Orca (2)").
#[must_use]
pub fn assign_unique_whale_name(id: &str, active_names: &HashSet<String>) -> String {
    let base = whale_name_for_id(id);
    if !active_names.contains(&base) {
        return base;
    }
    // Deterministic suffix from the same hash to keep it stable
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    id.hash(&mut hasher);
    let suffix_seed = hasher.finish();
    for i in 2.. {
        let candidate = format!("{base} ({i})");
        if !active_names.contains(&candidate) {
            return candidate;
        }
        // Vary the probe using the seed
        let probe = (suffix_seed.wrapping_add(i as u64)) % 100;
        let candidate2 = format!("{base} ({probe})");
        if !active_names.contains(&candidate2) {
            return candidate2;
        }
    }
    // Fallback (should never reach here)
    format!("{base} ({})", id.get(..4).unwrap_or("?"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_whale_name_deterministic() {
        let name1 = whale_name_for_id("agent-123");
        let name2 = whale_name_for_id("agent-123");
        assert_eq!(name1, name2);
    }

    #[test]
    fn test_assign_unique_whale_name_no_collision() {
        let mut active = HashSet::new();
        let name = assign_unique_whale_name("agent-123", &active);
        assert!(!name.is_empty());
        active.insert(name);
    }

    #[test]
    fn test_assign_unique_whale_name_with_collision() {
        let mut active = HashSet::new();
        let name1 = assign_unique_whale_name("agent-123", &active);
        active.insert(name1.clone());
        let name2 = assign_unique_whale_name("agent-456", &active);
        assert_ne!(name1, name2);
    }
}
