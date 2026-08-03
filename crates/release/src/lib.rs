use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

/// Filename of the SHA-256 checksum manifest included in every release.
///
/// Mirror directories must contain this file alongside platform binaries so
/// that download integrity can be verified.
pub const CHECKSUM_MANIFEST_ASSET: &str = "mimofan-artifacts-sha256.txt";

/// GitHub API URL for the single latest stable release.
pub const LATEST_RELEASE_URL: &str =
    "https://api.github.com/repos/XiaomingX/mimofan/releases/latest";

/// GitHub API URL listing recent releases (up to 100), used to find beta tags.
pub const RELEASES_URL: &str =
    "https://api.github.com/repos/XiaomingX/mimofan/releases?per_page=100";

/// Environment variable that overrides the base URL for release asset downloads.
pub const RELEASE_BASE_URL_ENV: &str = "MIMOFAN_RELEASE_BASE_URL";

/// Environment variable that pins the update target version.
pub const UPDATE_VERSION_ENV: &str = "MIMOFAN_VERSION";

/// User-Agent header sent with release metadata requests.
pub const UPDATE_USER_AGENT: &str = "mimofan-updater";

const RELEASE_METADATA_TIMEOUT: Duration = Duration::from_secs(5);

/// The release channel to query for updates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseChannel {
    /// Official stable releases only.
    Stable,
    /// Pre-release / beta versions.
    Beta,
}

impl ReleaseChannel {
    /// Creates a channel from a boolean flag (`true` → [`Beta`](Self::Beta)).
    pub fn from_beta_flag(beta: bool) -> Self {
        if beta { Self::Beta } else { Self::Stable }
    }

    /// Returns a lowercase human-readable label (`"stable"` or `"beta"`).
    pub fn label(self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Beta => "beta",
        }
    }
}

/// Describes where to fetch release metadata from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReleaseQuery {
    /// Use a custom mirror base URL and a pinned version.
    Mirror { base_url: String, version: String },
    /// Query the GitHub single-latest-release endpoint.
    GitHubLatest { url: &'static str },
    /// Query the GitHub release-list endpoint (used for beta discovery).
    GitHubReleaseList { url: &'static str },
}

/// Determines the appropriate [`ReleaseQuery`] for the given channel, taking
/// environment-variable overrides (mirror URL, pinned version) into account.
pub fn resolve_release_query(channel: ReleaseChannel) -> ReleaseQuery {
    let version = update_version_from_env().unwrap_or_else(|| env!("CARGO_PKG_VERSION").into());
    if let Some(base_url) = release_base_url_from_env() {
        return ReleaseQuery::Mirror { base_url, version };
    }

    match channel {
        ReleaseChannel::Stable => ReleaseQuery::GitHubLatest {
            url: LATEST_RELEASE_URL,
        },
        ReleaseChannel::Beta => ReleaseQuery::GitHubReleaseList { url: RELEASES_URL },
    }
}

/// Reads the release base URL from environment variables.
/// Returns `None` when no override is configured.
pub fn release_base_url_from_env() -> Option<String> {
    if let Ok(value) = std::env::var(RELEASE_BASE_URL_ENV) {
        let trimmed = value.trim().to_string();
        if !trimmed.is_empty() {
            return Some(trimmed);
        }
    }
    None
}

/// Returns the pinned update version from environment variables, or `None`
/// if `MIMOFAN_VERSION` is not set or empty.
pub fn update_version_from_env() -> Option<String> {
    std::env::var(UPDATE_VERSION_ENV)
        .ok()
        .map(|value| value.trim().trim_start_matches('v').to_string())
        .filter(|value| !value.is_empty())
}

/// Joins a mirror base URL with an asset filename to produce a full download URL.
pub fn mirror_asset_url(base_url: &str, asset_name: &str) -> String {
    format!("{}/{}", base_url.trim_end_matches('/'), asset_name)
}

/// Returns a human-readable hint explaining how to use a mirror when GitHub
/// downloads are blocked or slow (e.g. on mainland China networks).
pub fn update_network_fallback_hint() -> String {
    format!(
        "GitHub release downloads may be blocked or slow on this network.\n\
         Use a binary asset mirror:\n\
           {RELEASE_BASE_URL_ENV}=https://<mirror>/<release-assets>/ {UPDATE_VERSION_ENV}=X.Y.Z mimofan update\n\
         The mirror directory must contain {CHECKSUM_MANIFEST_ASSET} and the platform binaries."
    )
}

/// Fetches a release JSON payload from `url` using an async HTTP client.
///
/// `description` is included in error messages to identify the request purpose.
pub async fn fetch_release_json_async(url: &str, description: &str) -> Result<String> {
    let client = reqwest::Client::builder()
        .user_agent(UPDATE_USER_AGENT)
        .timeout(RELEASE_METADATA_TIMEOUT)
        .build()
        .context("failed to build release check HTTP client")?;
    let response = client
        .get(url)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .await
        .with_context(|| format!("failed to fetch {description} from {url}"))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .with_context(|| format!("failed to read {description} response from {url}"));
    release_response_body(status, body, url, description)
}

fn release_response_body(
    status: reqwest::StatusCode,
    body: Result<String>,
    url: &str,
    description: &str,
) -> Result<String> {
    let body = body.with_context(|| format!("failed to read {description} response from {url}"))?;
    if !status.is_success() {
        bail!("GitHub release request failed with HTTP {status}: {body}");
    }
    Ok(body)
}

#[derive(Deserialize)]
struct ReleaseTag {
    tag_name: String,
}

#[derive(Deserialize)]
struct ReleaseListEntry {
    tag_name: String,
}

/// Extracts the `tag_name` field from a GitHub single-release JSON response.
pub fn latest_tag_from_release_json(body: &str) -> Result<String> {
    let release: ReleaseTag = serde_json::from_str(body).with_context(|| {
        format!("failed to parse release JSON from GitHub API. Response: {body}")
    })?;
    Ok(release.tag_name)
}

/// Scans a GitHub release-list JSON response and returns the tag of the first
/// entry whose name contains `"beta"`.
pub fn latest_beta_tag_from_release_list_json(body: &str) -> Result<String> {
    let releases: Vec<ReleaseListEntry> = serde_json::from_str(body).with_context(|| {
        format!("failed to parse release list JSON from GitHub API. Response: {body}")
    })?;
    releases
        .into_iter()
        .find(|release| is_beta_tag(&release.tag_name))
        .map(|release| release.tag_name)
        .context("no beta release found in GitHub releases")
}

/// Async helper that resolves the latest release tag for the given channel.
///
/// For mirrors the version is derived from the pinned environment variable;
/// for GitHub channels the appropriate API endpoint is queried.
pub async fn latest_release_tag_async(channel: ReleaseChannel) -> Result<String> {
    match resolve_release_query(channel) {
        ReleaseQuery::Mirror { version, .. } => Ok(format!("v{}", version.trim_start_matches('v'))),
        ReleaseQuery::GitHubLatest { url } => {
            let body = fetch_release_json_async(url, "latest release").await?;
            latest_tag_from_release_json(&body)
        }
        ReleaseQuery::GitHubReleaseList { url } => {
            let body = fetch_release_json_async(url, "release list").await?;
            latest_beta_tag_from_release_list_json(&body)
        }
    }
}

/// Compares a current version string against a release tag using semver
/// ordering. Both `v` prefixes and trailing build metadata (e.g. `(abc123)`)
/// are stripped before comparison.
pub fn compare_release_versions(
    current_version: &str,
    latest_tag: &str,
) -> Result<std::cmp::Ordering> {
    let current = parse_release_version(current_version)
        .with_context(|| format!("failed to parse current version {current_version:?}"))?;
    let latest = parse_release_version(latest_tag)
        .with_context(|| format!("failed to parse latest release tag {latest_tag:?}"))?;
    Ok(current.cmp(&latest))
}

/// Determines whether an update is needed for the given channel.
///
/// For [`Stable`](ReleaseChannel::Stable) an update is needed when the latest
/// release is strictly newer. For [`Beta`](ReleaseChannel::Beta) the logic also
/// allows switching from a stable release to a beta on the same release line.
pub fn update_is_needed(
    channel: ReleaseChannel,
    current_version: &str,
    latest_tag: &str,
) -> Result<bool> {
    let current = parse_release_version(current_version)
        .with_context(|| format!("failed to parse current version {current_version:?}"))?;
    let latest = parse_release_version(latest_tag)
        .with_context(|| format!("failed to parse latest release tag {latest_tag:?}"))?;

    match channel {
        ReleaseChannel::Stable => Ok(current < latest),
        ReleaseChannel::Beta => {
            if current == latest {
                return Ok(false);
            }
            let latest_is_beta = version_is_beta(&latest);
            let current_is_stable = current.pre.is_empty();
            let same_release_line = current.major == latest.major
                && current.minor == latest.minor
                && current.patch == latest.patch;
            if current > latest && !(current_is_stable && same_release_line) {
                return Ok(false);
            }
            Ok(latest_is_beta)
        }
    }
}

/// Parses a version string (with optional `v` prefix and trailing build info)
/// into a [`semver::Version`].
pub fn parse_release_version(value: &str) -> Result<semver::Version> {
    let version = value
        .trim()
        .trim_start_matches('v')
        .split_whitespace()
        .next()
        .unwrap_or("");
    semver::Version::parse(version).with_context(|| format!("invalid semver: {value:?}"))
}

/// Returns `true` if the tag name contains `"beta"` (case-insensitive).
pub fn is_beta_tag(tag_name: &str) -> bool {
    tag_name.to_ascii_lowercase().contains("beta")
}

fn version_is_beta(version: &semver::Version) -> bool {
    version.pre.as_str().to_ascii_lowercase().contains("beta")
}

