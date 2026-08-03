//! 启动期版本检查：对比远端 release 与当前版本，生成升级提示。
//!
//! 该子系统自包含，仅依赖 `mimofan_release` 与 `serde_json`，不触碰事件循环内部状态。
use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
enum StartupVersionCheckSource {
    Disabled,
    ConfiguredUrl(String),
    ReleaseResolver,
}

pub(crate) fn spawn_startup_version_check(
    config: UpdateConfig,
) -> Option<tokio::task::JoinHandle<Option<String>>> {
    let source = startup_version_check_source(&config);
    if source == StartupVersionCheckSource::Disabled {
        return None;
    }

    let current = env!("CARGO_PKG_VERSION").to_string();
    Some(tokio::spawn(async move {
        version_hint_from_startup_source(source, &current).await
    }))
}

fn startup_version_check_source(config: &UpdateConfig) -> StartupVersionCheckSource {
    if !config.check_for_updates {
        return StartupVersionCheckSource::Disabled;
    }
    if let Some(update_uri) = config.update_uri() {
        return StartupVersionCheckSource::ConfiguredUrl(update_uri.to_string());
    }
    StartupVersionCheckSource::ReleaseResolver
}

async fn version_hint_from_startup_source(
    source: StartupVersionCheckSource,
    current: &str,
) -> Option<String> {
    match source {
        StartupVersionCheckSource::Disabled => None,
        StartupVersionCheckSource::ConfiguredUrl(url) => {
            match version_hint_from_configured_update_uri(&url, current).await {
                Ok(hint) => hint,
                Err(_) => version_hint_from_release_mirror_env(current).await,
            }
        }
        StartupVersionCheckSource::ReleaseResolver => {
            if release_mirror_env_configured() {
                return version_hint_from_release_mirror_env(current).await;
            }

            let body = mimofan_release::fetch_release_json_async(
                mimofan_release::LATEST_RELEASE_URL,
                "latest release",
            )
            .await
            .ok()?;
            let json: serde_json::Value = serde_json::from_str(&body).ok()?;
            version_hint_from_release_json(&json, current)
        }
    }
}

async fn version_hint_from_release_mirror_env(current: &str) -> Option<String> {
    if !release_mirror_env_configured() {
        return None;
    }
    let tag = mimofan_release::latest_release_tag_async(mimofan_release::ReleaseChannel::Stable)
        .await
        .ok()?;
    version_hint_from_latest_tag(&tag, current)
}

fn release_mirror_env_configured() -> bool {
    mimofan_release::release_base_url_from_env().is_some()
}

async fn version_hint_from_configured_update_uri(
    update_uri: &str,
    current: &str,
) -> Result<Option<String>> {
    let body =
        mimofan_release::fetch_release_json_async(update_uri, "configured latest release").await?;
    let json: serde_json::Value = serde_json::from_str(&body).with_context(|| {
        format!("failed to parse release JSON from configured URI {update_uri}")
    })?;
    Ok(version_hint_from_custom_release_json(&json, current))
}

fn version_hint_from_release_json(json: &serde_json::Value, current: &str) -> Option<String> {
    if !release_has_required_assets(json) {
        return None;
    }

    let tag = json["tag_name"].as_str()?;
    version_hint_from_latest_tag(tag, current)
}

fn version_hint_from_custom_release_json(
    json: &serde_json::Value,
    current: &str,
) -> Option<String> {
    if !release_is_publishable(json) {
        return None;
    }
    if json.get("assets").is_some() && !release_has_required_assets(json) {
        return None;
    }
    let tag = json["tag_name"].as_str()?;
    version_hint_from_latest_tag(tag, current)
}

fn version_hint_from_latest_tag(tag: &str, current: &str) -> Option<String> {
    let latest = tag.trim_start_matches('v');
    if !is_newer_version(latest, current) {
        return None;
    }

    Some(format!(
        "新版本 v{latest} 已发布，请运行 mimofan update 升级后重启"
    ))
}

fn release_has_required_assets(json: &serde_json::Value) -> bool {
    if !release_is_publishable(json) {
        return false;
    }

    super::REQUIRED_RELEASE_ASSETS
        .iter()
        .all(|required| release_has_uploaded_asset(json, required))
}

fn release_is_publishable(json: &serde_json::Value) -> bool {
    !json
        .get("draft")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
        && !json
            .get("prerelease")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
}

fn release_has_uploaded_asset(json: &serde_json::Value, required: &str) -> bool {
    let Some(assets) = json.get("assets").and_then(serde_json::Value::as_array) else {
        return false;
    };
    assets.iter().any(|asset| {
        asset.get("name").and_then(serde_json::Value::as_str) == Some(required)
            && asset.get("state").and_then(serde_json::Value::as_str) == Some("uploaded")
    })
}

fn is_newer_version(latest: &str, current: &str) -> bool {
    // Compare semver so dev builds (e.g. "0.8.46-pre") don't trigger false
    // hints. Falls back to string compare on unparseable versions.
    match (parse_semver(latest), parse_semver(current)) {
        (Some(l), Some(c)) => l > c,
        _ => latest != current,
    }
}

/// Parse a `major.minor.patch` version string into a comparable tuple.
/// Returns `None` on any parse failure (non-semver, dev suffixes, etc.).
fn parse_semver(v: &str) -> Option<(u32, u32, u32)> {
    let mut parts = v.splitn(3, '.');
    let major = parts.next()?.parse::<u32>().ok()?;
    let minor = parts.next()?.parse::<u32>().ok()?;
    let patch = parts.next().unwrap_or("0").parse::<u32>().ok()?;
    Some((major, minor, patch))
}
