use mimofan_release::*;
use std::ffi::OsString;
use std::sync::{Mutex, MutexGuard};

static RELEASE_ENV_LOCK: Mutex<()> = Mutex::new(());
const RELEASE_ENV_VARS: &[&str] = &[RELEASE_BASE_URL_ENV, UPDATE_VERSION_ENV];

struct ReleaseEnvGuard {
    previous: Vec<(&'static str, Option<OsString>)>,
    _lock: MutexGuard<'static, ()>,
}

impl ReleaseEnvGuard {
    fn clear() -> Self {
        let lock = RELEASE_ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous = RELEASE_ENV_VARS
            .iter()
            .map(|&name| (name, std::env::var_os(name)))
            .collect();
        for &name in RELEASE_ENV_VARS {
            // SAFETY: tests that mutate these process-wide vars hold RELEASE_ENV_LOCK.
            unsafe { std::env::remove_var(name) };
        }
        Self {
            previous,
            _lock: lock,
        }
    }
}

impl Drop for ReleaseEnvGuard {
    fn drop(&mut self) {
        for (name, value) in &self.previous {
            // SAFETY: the guard still holds RELEASE_ENV_LOCK while restoring state.
            unsafe {
                match value {
                    Some(value) => std::env::set_var(name, value),
                    None => std::env::remove_var(name),
                }
            }
        }
    }
}

fn set_release_env(name: &str, value: &str) {
    // SAFETY: callers hold a ReleaseEnvGuard, which serializes env mutation.
    unsafe { std::env::set_var(name, value) };
}

#[test]
fn release_channel_from_beta_flag_maps_booleans() {
    assert_eq!(
        ReleaseChannel::from_beta_flag(false),
        ReleaseChannel::Stable
    );
    assert_eq!(ReleaseChannel::from_beta_flag(true), ReleaseChannel::Beta);
}

#[test]
fn release_channel_label_matches_channel_names() {
    assert_eq!(ReleaseChannel::Stable.label(), "stable");
    assert_eq!(ReleaseChannel::Beta.label(), "beta");
}

#[test]
fn is_beta_tag_detects_beta_prereleases_case_insensitively() {
    for tag in [
        "beta",
        "BETA",
        "BeTa",
        "v1.0.0-beta.1",
        "v1.0.0-BETA.1",
        "v2.0.0-beta",
        "something-beta-something",
        "beta-1.0",
    ] {
        assert!(is_beta_tag(tag), "{tag} should be beta");
    }

    for tag in ["", "bet", "alpha", "rc", "v1.0.0", "v1.0.0-alpha.1", "v1.0.0-rc.1"] {
        assert!(!is_beta_tag(tag), "{tag} should not be beta");
    }
}

#[test]
fn release_base_url_from_env_returns_none_without_overrides() {
    let _env = ReleaseEnvGuard::clear();

    assert_eq!(release_base_url_from_env(), None);
}

#[test]
fn release_base_url_from_env_uses_primary_override() {
    let _env = ReleaseEnvGuard::clear();
    set_release_env(RELEASE_BASE_URL_ENV, "https://primary.example.com");

    assert_eq!(
        release_base_url_from_env(),
        Some("https://primary.example.com".to_string())
    );
}

#[test]
fn release_base_url_from_env_trims_and_ignores_empty_overrides() {
    let _env = ReleaseEnvGuard::clear();
    set_release_env(RELEASE_BASE_URL_ENV, "  https://spaced.example.com  \n");

    assert_eq!(
        release_base_url_from_env(),
        Some("https://spaced.example.com".to_string())
    );

    set_release_env(RELEASE_BASE_URL_ENV, "   ");

    assert_eq!(release_base_url_from_env(), None);
}

#[test]
fn update_version_from_env_trims_and_strips_v_prefix() {
    let _env = ReleaseEnvGuard::clear();
    set_release_env(UPDATE_VERSION_ENV, "  v1.2.3  ");

    assert_eq!(update_version_from_env().as_deref(), Some("1.2.3"));
}

#[test]
fn update_version_from_env_ignores_missing_or_empty_values() {
    let _env = ReleaseEnvGuard::clear();
    assert_eq!(update_version_from_env(), None);

    set_release_env(UPDATE_VERSION_ENV, "   ");

    assert_eq!(update_version_from_env(), None);
}

#[test]
fn update_network_fallback_hint_mentions_required_mirror_inputs() {
    let hint = update_network_fallback_hint();

    assert!(
        hint.contains(RELEASE_BASE_URL_ENV),
        "hint missing RELEASE_BASE_URL_ENV"
    );
    assert!(
        hint.contains(UPDATE_VERSION_ENV),
        "hint missing UPDATE_VERSION_ENV"
    );
    assert!(
        hint.contains(CHECKSUM_MANIFEST_ASSET),
        "hint missing CHECKSUM_MANIFEST_ASSET"
    );
}

#[test]
fn mirror_asset_url_trims_trailing_base_slashes() {
    for base_url in [
        "https://example.com/assets",
        "https://example.com/assets/",
        "https://example.com/assets//",
    ] {
        assert_eq!(
            mirror_asset_url(base_url, "file.zip"),
            "https://example.com/assets/file.zip",
            "{base_url} should join with a single slash"
        );
    }
    assert_eq!(mirror_asset_url("", "file.zip"), "/file.zip");
}

#[test]
fn resolve_release_query_uses_github_without_overrides() {
    let _env = ReleaseEnvGuard::clear();

    assert_eq!(
        resolve_release_query(ReleaseChannel::Stable),
        ReleaseQuery::GitHubLatest {
            url: LATEST_RELEASE_URL
        }
    );
    assert_eq!(
        resolve_release_query(ReleaseChannel::Beta),
        ReleaseQuery::GitHubReleaseList { url: RELEASES_URL }
    );
}

#[test]
fn resolve_release_query_uses_release_base_url_overrides() {
    let default_version = env!("CARGO_PKG_VERSION").to_string();
    let _env = ReleaseEnvGuard::clear();
    set_release_env(RELEASE_BASE_URL_ENV, "https://primary.example.com/mirror");

    assert_eq!(
        resolve_release_query(ReleaseChannel::Stable),
        ReleaseQuery::Mirror {
            base_url: "https://primary.example.com/mirror".to_string(),
            version: default_version,
        }
    );
}

#[test]
fn resolve_release_query_uses_pinned_release_versions_for_mirrors() {
    let _env = ReleaseEnvGuard::clear();
    set_release_env(RELEASE_BASE_URL_ENV, "https://example.com/mirror");
    set_release_env(UPDATE_VERSION_ENV, "v1.2.3");

    assert_eq!(
        resolve_release_query(ReleaseChannel::Stable),
        ReleaseQuery::Mirror {
            base_url: "https://example.com/mirror".to_string(),
            version: "1.2.3".to_string(),
        }
    );
}

#[test]
fn stable_update_is_needed_only_when_latest_is_newer() {
    assert!(update_is_needed(ReleaseChannel::Stable, "0.8.45", "v0.8.46").expect("stable_update_is_needed_only_when_latest_is_newer"));
    assert!(update_is_needed(ReleaseChannel::Stable, "0.8.45", "v0.9.0-beta.1").expect("stable_update_is_needed_only_when_latest_is_newer"));
    assert!(!update_is_needed(ReleaseChannel::Stable, "0.8.45", "v0.8.45").expect("stable_update_is_needed_only_when_latest_is_newer"));
    assert!(!update_is_needed(ReleaseChannel::Stable, "0.9.0", "v0.9.0-beta.1").expect("stable_update_is_needed_only_when_latest_is_newer"));
    assert!(
        !update_is_needed(ReleaseChannel::Stable, "0.9.0-beta.2", "v0.9.0-beta.1").expect("stable_update_is_needed_only_when_latest_is_newer")
    );
}

#[test]
fn beta_update_allows_switching_from_same_stable_to_beta() {
    assert!(update_is_needed(ReleaseChannel::Beta, "1.0.0", "v1.0.0-beta.2").expect("beta_update_allows_switching_from_same_stable_to_beta"));
    assert!(!update_is_needed(ReleaseChannel::Beta, "1.0.0-beta.2", "v1.0.0-beta.2").expect("beta_update_allows_switching_from_same_stable_to_beta"));
    assert!(!update_is_needed(ReleaseChannel::Beta, "1.0.0-beta.3", "v1.0.0-beta.2").expect("beta_update_allows_switching_from_same_stable_to_beta"));
    assert!(update_is_needed(ReleaseChannel::Beta, "1.0.0-beta.2", "v1.0.0-beta.3").expect("beta_update_allows_switching_from_same_stable_to_beta"));
    assert!(!update_is_needed(ReleaseChannel::Beta, "2.0.0", "v1.0.0-beta.3").expect("beta_update_allows_switching_from_same_stable_to_beta"));
    assert!(!update_is_needed(ReleaseChannel::Beta, "1.0.0-rc.1", "v1.0.0-beta.3").expect("beta_update_allows_switching_from_same_stable_to_beta"));
}

#[test]
fn parse_release_version_accepts_tags_and_build_suffixes() {
    assert_eq!(
        parse_release_version("v0.9.0-beta.1").expect("parse_release_version_accepts_tags_and_build_suffixes"),
        semver::Version::parse("0.9.0-beta.1").expect("parse_release_version_accepts_tags_and_build_suffixes")
    );
    assert_eq!(
        parse_release_version("0.8.45 (abcdef123456)").expect("parse_release_version_accepts_tags_and_build_suffixes"),
        semver::Version::parse("0.8.45").expect("parse_release_version_accepts_tags_and_build_suffixes")
    );
}

#[test]
fn release_version_compare_ignores_v_prefix_and_build_sha() {
    assert_eq!(
        compare_release_versions("0.8.39 (eeccf7d)", "v0.8.39").expect("release_version_compare_ignores_v_prefix_and_build_sha"),
        std::cmp::Ordering::Equal
    );
    assert_eq!(
        compare_release_versions("0.8.39", "v0.8.40").expect("release_version_compare_ignores_v_prefix_and_build_sha"),
        std::cmp::Ordering::Less
    );
    assert_eq!(
        compare_release_versions("0.8.40", "v0.8.39").expect("release_version_compare_ignores_v_prefix_and_build_sha"),
        std::cmp::Ordering::Greater
    );
}

#[test]
fn latest_beta_tag_selects_first_beta_release() {
    let body = r#"[
      { "tag_name": "v0.9.0" },
      { "tag_name": "v0.9.0-rc.1" },
      { "tag_name": "v0.9.0-beta.2" },
      { "tag_name": "v0.9.0-beta.1" }
    ]"#;
    assert_eq!(
        latest_beta_tag_from_release_list_json(body).expect("latest_beta_tag_selects_first_beta_release"),
        "v0.9.0-beta.2"
    );
}

#[test]
fn latest_beta_tag_reports_missing_beta() {
    let body = r#"[{ "tag_name": "v0.9.0" }]"#;
    let err = latest_beta_tag_from_release_list_json(body).expect_err("missing beta");
    assert!(
        err.to_string().contains("no beta release found"),
        "unexpected error: {err:#}"
    );
}
