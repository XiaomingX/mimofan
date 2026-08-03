//! Externalized integration tests for `mimofan_config::provider`.
//!
//! Relocated verbatim from `crates/config/src/provider.rs`. Only the
//! `#[cfg(test)] mod tests` wrapper and the `use super::*` import were replaced
//! with the public-API imports below; no test logic or assertion changed.

use mimofan_config::ProviderKind;
use mimofan_config::provider::*;

#[test]
fn display_order_is_alphabetical_by_display_name() {
    let display = providers_sorted_for_display();
    let names: Vec<String> = display
        .iter()
        .map(|p| p.display_name().to_ascii_lowercase())
        .collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(
        names, sorted,
        "providers_sorted_for_display must be alphabetical (case-insensitive) by display name"
    );
}

#[test]
fn display_order_is_complete_and_unique() {
    let display = providers_sorted_for_display();
    assert_eq!(
        display.len(),
        all_providers().len(),
        "display order must include every built-in provider"
    );
    let mut ids: Vec<&str> = display.iter().map(|p| p.id()).collect();
    ids.sort_unstable();
    let before = ids.len();
    ids.dedup();
    assert_eq!(
        before,
        ids.len(),
        "display order must not contain duplicates"
    );
}

#[test]
fn xiaomi_mimo_and_custom_present() {
    let providers = all_providers();
    assert!(
        providers
            .iter()
            .any(|p| p.kind() == ProviderKind::XiaomiMimo)
    );
    assert!(
        providers
            .iter()
            .any(|p| p.kind() == ProviderKind::Anthropic)
    );
    assert!(providers.iter().any(|p| p.kind() == ProviderKind::Custom));
}
