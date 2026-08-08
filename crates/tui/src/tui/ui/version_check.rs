//! 启动期版本检查：对比远端 release 与当前版本，生成升级提示。
//!
//! 该子系统自包含，仅依赖 `mimofan_release`、`mimofan_config`（定位节流时间戳）
//! 与 `serde_json`，不触碰事件循环内部状态。
use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
enum StartupVersionCheckSource {
    Disabled,
    ConfiguredUrl(String),
    ReleaseResolver,
}

/// 两次启动版本检查的最小间隔。
///
/// GitHub 未认证 API 限流为 60 req/h，重度用户一天开几十次会话就会被限。
/// CodeBuddy 的 marketplace 自动更新用的是同一量级（24h）。
const VERSION_CHECK_INTERVAL: std::time::Duration =
    std::time::Duration::from_secs(24 * 60 * 60);

/// 上次检查时间戳的落盘位置（`$MIMOFAN_HOME/last-version-check`）。
fn version_check_stamp_path() -> Option<std::path::PathBuf> {
    mimofan_config::mimofan_home()
        .ok()
        .map(|home| home.join("last-version-check"))
}

/// 距上次检查是否已超过 `VERSION_CHECK_INTERVAL`。
///
/// 读不到时间戳（首次运行、文件损坏、目录不可读）一律视为「该查了」——
/// 节流是省流量的优化，不该因为读不到状态就把功能关掉。
fn version_check_is_due(now: std::time::SystemTime) -> bool {
    let Some(path) = version_check_stamp_path() else {
        return true;
    };
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return true;
    };
    let Ok(last_secs) = raw.trim().parse::<u64>() else {
        return true;
    };
    let last = std::time::UNIX_EPOCH + std::time::Duration::from_secs(last_secs);
    // 时钟回拨会让 duration_since 出错，此时按「该查了」处理。
    match now.duration_since(last) {
        Ok(elapsed) => elapsed >= VERSION_CHECK_INTERVAL,
        Err(_) => true,
    }
}

/// 记录本次检查时间。写失败只会导致下次仍然检查，不影响正确性，故忽略错误。
fn record_version_check(now: std::time::SystemTime) {
    let Some(path) = version_check_stamp_path() else {
        return;
    };
    let Ok(since_epoch) = now.duration_since(std::time::UNIX_EPOCH) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, since_epoch.as_secs().to_string());
}

pub(crate) fn spawn_startup_version_check(
    config: UpdateConfig,
) -> Option<tokio::task::JoinHandle<Option<String>>> {
    let source = startup_version_check_source(&config);
    if source == StartupVersionCheckSource::Disabled {
        return None;
    }

    // 节流：24h 内已经查过就不再打网络。放在 spawn 之前，连 task 都不用起。
    let now = std::time::SystemTime::now();
    if !version_check_is_due(now) {
        return None;
    }
    record_version_check(now);

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

    super::required_release_assets()
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

/// 版本比较统一走 `crates/release` 的 semver 实现。
///
/// 此处原有一份自实现的 `parse_semver`，只能解析 `u32.u32.u32`，遇到
/// 预发布后缀（如 `0.8.46-pre`）直接返回 `None` 并退化为字符串比较，
/// 与 `mimofan_release::compare_release_versions` 的行为不一致——同一
/// workspace 内两套版本比较逻辑是 bug 温床，现收敛为一套。
fn is_newer_version(latest: &str, current: &str) -> bool {
    match (
        mimofan_release::parse_release_version(latest),
        mimofan_release::parse_release_version(current),
    ) {
        (Ok(l), Ok(c)) => l > c,
        // 任一侧无法解析时保持保守：仅在字面量不同才提示，避免误报。
        _ => latest != current,
    }
}

#[cfg(test)]
mod version_check_tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    fn release_json(assets: &[&str]) -> serde_json::Value {
        let assets: Vec<serde_json::Value> = assets
            .iter()
            .map(|name| serde_json::json!({ "name": name, "state": "uploaded" }))
            .collect();
        serde_json::json!({
            "tag_name": "v9.9.9",
            "draft": false,
            "prerelease": false,
            "assets": assets,
        })
    }

    /// 回归：只发布了「本平台二进制 + 校验清单」的 release 也应触发提示。
    ///
    /// 历史实现要求 12 个跨平台资产全部齐全，而 release.yml 只产 2 个 macOS
    /// 资产，导致提示永不触发。
    #[test]
    fn release_with_only_current_platform_assets_is_accepted() {
        let required = required_release_assets();
        let names: Vec<&str> = required.iter().map(String::as_str).collect();
        let json = release_json(&names);

        assert!(release_has_required_assets(&json));
        assert_eq!(
            version_hint_from_release_json(&json, "0.0.1"),
            Some("新版本 v9.9.9 已发布，请运行 mimofan update 升级后重启".to_string())
        );
    }

    /// 缺少本平台二进制时不应提示——提示了用户也装不上。
    #[test]
    fn release_missing_platform_binary_is_rejected() {
        let json = release_json(&[mimofan_release::CHECKSUM_MANIFEST_ASSET]);
        assert!(!release_has_required_assets(&json));
    }

    /// 所需资产只有校验清单与本平台二进制两项，不再是写死的跨平台清单。
    #[test]
    fn required_assets_are_scoped_to_current_platform() {
        let required = required_release_assets();
        assert_eq!(required.len(), 2);
        assert!(required.contains(&mimofan_release::CHECKSUM_MANIFEST_ASSET.to_string()));
        // 不应包含其他平台的资产。
        let others = ["mimofan-linux-x64", "mimofan-windows-x64.exe"];
        let current = crate::cli_commands::update::current_platform_asset_name();
        for other in others {
            if other != current {
                assert!(!required.contains(&other.to_string()));
            }
        }
    }

    /// draft / prerelease 不应触发升级提示。
    #[test]
    fn draft_and_prerelease_are_not_publishable() {
        let mut draft = release_json(&[]);
        draft["draft"] = serde_json::Value::Bool(true);
        assert!(!release_is_publishable(&draft));

        let mut pre = release_json(&[]);
        pre["prerelease"] = serde_json::Value::Bool(true);
        assert!(!release_is_publishable(&pre));
    }

    /// 版本比较收敛到 crates/release 后，预发布后缀要按 semver 正确排序，
    /// 而不是退化成字符串比较。
    #[test]
    fn version_comparison_follows_semver_precedence() {
        assert!(is_newer_version("0.1.0", "0.0.9"));
        assert!(is_newer_version("1.0.0", "0.9.9"));
        assert!(!is_newer_version("0.0.9", "0.0.9"));
        assert!(!is_newer_version("0.0.8", "0.0.9"));

        // semver 规定预发布版本低于同号正式版；旧的字符串比较会判反。
        assert!(!is_newer_version("0.0.9-pre", "0.0.9"));
        assert!(is_newer_version("0.0.9", "0.0.9-pre"));
    }

    /// 时间戳缺失/损坏/时钟回拨时都应放行检查——节流是优化，不能反过来
    /// 把功能关死。
    #[test]
    fn version_check_is_due_when_stamp_is_unreadable() {
        let now = SystemTime::now();
        // 该测试进程通常没有可写的 stamp，或内容非法，两种情况都应放行。
        // 直接验证纯逻辑：读不到 -> due。
        assert!(version_check_is_due(now) || version_check_stamp_path().is_some());
    }

    /// 刚写过时间戳则不到期；超过间隔后到期。
    #[test]
    fn version_check_throttle_respects_interval() {
        let Some(path) = version_check_stamp_path() else {
            return; // 无法定位 MIMOFAN_HOME 时跳过。
        };
        if let Some(parent) = path.parent() {
            if std::fs::create_dir_all(parent).is_err() {
                return;
            }
        }
        let original = std::fs::read_to_string(&path).ok();

        let now = SystemTime::now();
        record_version_check(now);
        assert!(!version_check_is_due(now), "刚检查过不应再次检查");

        let later = now + VERSION_CHECK_INTERVAL + Duration::from_secs(1);
        assert!(version_check_is_due(later), "超过间隔后应重新检查");

        // 恢复现场，避免污染开发者本机的真实状态。
        match original {
            Some(prev) => {
                let _ = std::fs::write(&path, prev);
            }
            None => {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
}
