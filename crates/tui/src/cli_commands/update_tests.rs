// Tests relocated from src/cli_commands/update.rs

use super::*;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::thread;

#[cfg(test)]
fn expected_sha256_from_manifest(text: &str, asset_name: &str) -> Result<String> {
    let checksums = parse_checksum_manifest(text)?;
    checksums
        .get(asset_name)
        .cloned()
        .with_context(|| format!("checksum manifest is missing {asset_name}"))
}

/// Verify the arch mapping used when constructing asset names.
/// The mapping must use release-asset naming (arm64/x64), not Rust
/// stdlib constants (aarch64/x86_64).
#[test]
fn test_arch_mapping() {
    assert_eq!(release_arch_for_rust_arch("aarch64"), "arm64");
    assert_eq!(release_arch_for_rust_arch("x86_64"), "x64");
    // Pass-through for unknown arches
    assert_eq!(release_arch_for_rust_arch("riscv64"), "riscv64");
    // The currently-compiled arch maps to a release asset name
    let compiled_arch = std::env::consts::ARCH;
    let asset_arch = release_arch_for_rust_arch(compiled_arch);
    // Must not contain the raw Rust constant names
    assert!(
        !asset_arch.contains("aarch64") && !asset_arch.contains("x86_64"),
        "asset arch '{asset_arch}' still uses raw Rust constant name"
    );
}

/// Verify binary prefix detection for dispatcher vs TUI binary.
#[test]
fn test_binary_prefix_detection() {
    // After the merge, all mimofan variants use "mimofan" prefix
    assert_eq!(binary_prefix_for_exe(Path::new("mimofan-tui")), "mimofan");
    assert_eq!(
        binary_prefix_for_exe(Path::new("mimofan-tui.exe")),
        "mimofan"
    );
    assert_eq!(
        binary_prefix_for_exe(Path::new("mimofan-TUI.exe")),
        "mimofan"
    );
    assert_eq!(
        binary_prefix_for_exe(Path::new("/usr/local/bin/mimofan-tui")),
        "mimofan"
    );

    // Dispatcher binary should use mimofan prefix
    assert_eq!(binary_prefix_for_exe(Path::new("mimofan")), "mimofan");
    assert_eq!(binary_prefix_for_exe(Path::new("mimofan.exe")), "mimofan");
    assert_eq!(
        binary_prefix_for_exe(Path::new("/usr/local/bin/mimofan")),
        "mimofan"
    );

    // Fallback for unknown names
    assert_eq!(binary_prefix_for_exe(Path::new("other-binary")), "mimofan");

    // Legacy names still map to the canonical update asset prefix.
    assert_eq!(binary_prefix_for_exe(Path::new("mimofan")), "mimofan");
    assert_eq!(
        binary_prefix_for_exe(Path::new("/usr/local/bin/mimofan")),
        "mimofan"
    );
    assert_eq!(binary_prefix_for_exe(Path::new("Mimofan.exe")), "mimofan");
    assert_eq!(binary_prefix_for_exe(Path::new("deepseek")), "mimofan");
}

#[test]
fn test_is_legacy_binary_detection() {
    // Only "deepseek*" binaries are considered legacy
    assert!(is_legacy_binary(Path::new("deepseek")));
    assert!(is_legacy_binary(Path::new("/usr/local/bin/deepseek")));
    assert!(is_legacy_binary(Path::new("DeepSeek.exe")));
    // "mimofan" is the current canonical name, not legacy
    assert!(!is_legacy_binary(Path::new("mimofan")));
    assert!(!is_legacy_binary(Path::new("/usr/local/bin/mimofan")));
    assert!(!is_legacy_binary(Path::new("Mimofan.exe")));
    assert!(!is_legacy_binary(Path::new("mimofan-tui")));
}

#[test]
fn legacy_binary_message_gives_copy_pasteable_migration_steps() {
    let message = legacy_binary_message(Path::new("/usr/local/bin/mimofan"));

    assert!(message.contains("legacy deepseek/mimofan command name"));
    assert!(message.contains("install the canonical"));
    assert!(message.contains("DeepSeek provider support"));
    assert!(message.contains("is unchanged"));
    assert!(message.contains("npm uninstall -g mimofan"));
    assert!(message.contains("npm install -g mimofan"));
    assert!(message.contains("cargo uninstall mimofan 2>/dev/null || true"));
    assert!(message.contains("cargo install mimofan --locked"));
    assert!(message.contains("brew upgrade mimofan"));
    assert!(message.contains("https://github.com/XiaomingX/mimofan/releases/latest"));
}

#[test]
fn legacy_dispatcher_update_targets_canonical_mimofan() {
    let dir = tempfile::TempDir::new().expect("create temp dir");
    let dispatcher = dir
        .path()
        .join(format!("deepseek{}", std::env::consts::EXE_SUFFIX));
    let mimofan_bin = dir
        .path()
        .join(format!("mimofan{}", std::env::consts::EXE_SUFFIX));
    std::fs::write(&dispatcher, b"legacy dispatcher").expect("write temp file");
    std::fs::write(&mimofan_bin, b"mimofan binary").expect("write temp file");

    let targets = update_targets_for_exe(&dispatcher);
    let paths = targets
        .iter()
        .map(|target| target.path.clone())
        .collect::<Vec<_>>();

    // Legacy deepseek dispatcher targets both itself and the mimofan companion
    assert_eq!(paths.len(), 2);
    assert!(targets[0].asset_stem.starts_with("mimofan-"));
    assert!(targets[1].asset_stem.starts_with("mimofan-"));
}

#[test]
fn mimofan_update_targets_only_itself() {
    let dir = tempfile::TempDir::new().expect("create temp dir");
    let mimofan = dir
        .path()
        .join(format!("mimofan{}", std::env::consts::EXE_SUFFIX));
    std::fs::write(&mimofan, b"mimofan binary").expect("write temp file");

    let targets = update_targets_for_exe(&mimofan);
    let paths = targets
        .iter()
        .map(|target| target.path.clone())
        .collect::<Vec<_>>();

    // After the merge, mimofan is a single binary - no sibling to update
    assert_eq!(paths, vec![mimofan]);
    assert!(targets[0].asset_stem.starts_with("mimofan-"));
}

#[test]
fn test_release_asset_stem_for_supported_platforms() {
    let cases = [
        ("mimofan", "macos", "aarch64", "mimofan-macos-arm64"),
        ("mimofan", "macos", "x86_64", "mimofan-macos-x64"),
        ("mimofan", "linux", "x86_64", "mimofan-linux-x64"),
        ("mimofan", "windows", "x86_64", "mimofan-windows-x64"),
    ];

    for (exe, os, arch, expected) in cases {
        assert_eq!(release_asset_stem_for(Path::new(exe), os, arch), expected);
    }
}

/// 发布矩阵与自更新消费端必须产出/期望同一批资产名。
///
/// 回归背景：release.yml 长期只构建 2 个 macOS 资产，而 update.rs 会按
/// `mimofan-{os}-{arch}` 去 release 里找本平台资产，导致 Linux/Windows 用户
/// 执行 `mimofan update` 直接报 "no asset found for platform"。此测试直接
/// 解析 workflow 里的 artifact_name，与消费端命名规则逐一对齐，防止两边再次漂移。
#[test]
fn release_workflow_publishes_every_platform_the_updater_expects() {
    let workflow = include_str!("../../../../.github/workflows/release.yml");

    let published: Vec<&str> = workflow
        .lines()
        .filter_map(|line| line.trim().strip_prefix("artifact_name:"))
        .map(str::trim)
        .collect();

    // 消费端 release_asset_name_for_prefix 的命名规则：windows 带 .exe。
    let expected = [
        ("macos", "x86_64"),
        ("macos", "aarch64"),
        ("linux", "x86_64"),
        ("linux", "aarch64"),
        ("windows", "x86_64"),
    ];

    for (os, arch) in expected {
        let stem = release_asset_stem_for(Path::new("mimofan"), os, arch);
        let asset = if os == "windows" {
            format!("{stem}.exe")
        } else {
            stem
        };
        assert!(
            published.contains(&asset.as_str()),
            "release.yml 未发布 {os}/{arch} 所需资产 `{asset}`；\
             该平台用户将无法 `mimofan update`。已发布：{published:?}"
        );
    }
}

#[test]
fn update_targets_include_only_mimofan() {
    let dir = tempfile::TempDir::new().expect("create temp dir");
    let mimofan = dir
        .path()
        .join(format!("mimofan{}", std::env::consts::EXE_SUFFIX));
    std::fs::write(&mimofan, b"mimofan").expect("write temp file");

    let targets = update_targets_for_exe(&mimofan);
    let paths = targets
        .iter()
        .map(|target| target.path.as_path())
        .collect::<Vec<_>>();

    // After the merge, only mimofan is updated
    assert_eq!(paths, vec![mimofan.as_path()]);
    assert!(targets[0].asset_stem.starts_with("mimofan-"));
}

#[test]
fn update_targets_skip_missing_sibling() {
    let dir = tempfile::TempDir::new().expect("create temp dir");
    let dispatcher = dir
        .path()
        .join(format!("mimofan{}", std::env::consts::EXE_SUFFIX));
    std::fs::write(&dispatcher, b"dispatcher").expect("write temp file");

    let targets = update_targets_for_exe(&dispatcher);

    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].path, dispatcher);
    assert!(targets[0].asset_stem.starts_with("mimofan-"));
}

#[test]
fn test_asset_matching_accepts_binary_assets_and_rejects_checksums() {
    assert!(asset_matches_platform(
        "mimofan-macos-arm64",
        "mimofan-macos-arm64"
    ));
    assert!(asset_matches_platform(
        "mimofan-macos-arm64.tar.gz",
        "mimofan-macos-arm64"
    ));
    assert!(asset_matches_platform(
        "mimofan-windows-x64.exe",
        "mimofan-windows-x64"
    ));
    assert!(!asset_matches_platform(
        "mimofan-windows-x64.exe.sha256",
        "mimofan-windows-x64"
    ));
    assert!(!asset_matches_platform(
        "mimofan-macos-aarch64.tar.gz",
        "mimofan-macos-arm64"
    ));
}

#[test]
fn test_sha256_hex_known_value() {
    let data = b"hello";
    let hash = sha256_hex(data);
    assert_eq!(
        hash,
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
    );
}

#[test]
fn test_sha256_hex_empty() {
    let hash = sha256_hex(b"");
    assert_eq!(
        hash,
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

#[test]
fn glibc_version_parser_reads_getconf_and_symbol_text() {
    assert_eq!(
        parse_glibc_version("glibc 2.35\n"),
        Some(GlibcVersion::new(2, 35, 0))
    );
    assert_eq!(
        parse_glibc_version("requires GLIBC_2.39"),
        Some(GlibcVersion::new(2, 39, 0))
    );
    assert_eq!(parse_glibc_version("not glibc"), None);
}

#[test]
fn highest_required_glibc_finds_highest_binary_symbol() {
    let bytes = b"\0GLIBC_2.17\0other\0GLIBC_2.39\0GLIBC_2.35";

    assert_eq!(
        highest_required_glibc(bytes),
        Some(GlibcVersion::new(2, 39, 0))
    );
}

#[test]
fn glibc_compatibility_message_is_mimofan_branded_and_actionable() {
    let message = glibc_compatibility_message(
        "mimofan-linux-x64",
        GlibcVersion::new(2, 39, 0),
        Some(GlibcVersion::new(2, 35, 0)),
    );

    assert!(message.contains("Prebuilt mimofan asset `mimofan-linux-x64`"));
    assert!(message.contains("requires GLIBC_2.39"));
    assert!(message.contains("this system has glibc 2.35"));
    assert!(message.contains("cargo install mimofan --locked"));
    assert!(message.contains("build Linux GNU assets against an older glibc"));
}

#[test]
fn parse_checksum_manifest_accepts_sha256sum_format() {
    let manifest = "\
2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824  mimofan-macos-arm64
E3B0C44298FC1C149AFBF4C8996FB92427AE41E4649B934CA495991B7852B855  *mimofan-windows-x64.exe
";
    let checksums = parse_checksum_manifest(manifest).expect("valid manifest");

    assert_eq!(
        checksums.get("mimofan-macos-arm64").map(String::as_str),
        Some("2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824")
    );
    assert_eq!(
        checksums.get("mimofan-windows-x64.exe").map(String::as_str),
        Some("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")
    );
}

#[test]
fn parse_checksum_manifest_rejects_malformed_lines() {
    let err = parse_checksum_manifest("not-a-hash  mimofan-macos-arm64")
        .expect_err("invalid manifest line should fail");
    assert!(
        err.to_string().contains("invalid SHA256 manifest line"),
        "unexpected error: {err:#}"
    );
}

#[test]
fn expected_sha256_from_manifest_requires_matching_asset() {
    let manifest =
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824  other-asset\n";
    let err = expected_sha256_from_manifest(manifest, "mimofan-macos-arm64")
        .expect_err("missing asset should fail");
    assert!(
        err.to_string()
            .contains("checksum manifest is missing mimofan-macos-arm64"),
        "unexpected error: {err:#}"
    );
}

#[test]
fn test_replace_binary_creates_and_replaces() {
    let dir = tempfile::TempDir::new().expect("create temp dir");
    let target = dir.path().join("mimofan-test");
    // Write initial content
    std::fs::write(&target, b"old binary").expect("write temp file");

    replace_binary(&target, b"new binary content").expect("replace binary");
    let content = std::fs::read_to_string(&target).expect("read replaced binary");
    assert_eq!(content, "new binary content");
}

#[test]
fn test_replace_binary_creates_new_file() {
    let dir = tempfile::TempDir::new().expect("create temp dir");
    let target = dir.path().join("mimofan-new-test");

    replace_binary(&target, b"fresh binary").expect("replace binary");
    let content = std::fs::read_to_string(&target).expect("read replaced binary");
    assert_eq!(content, "fresh binary");
}

/// Mocked GitHub release payload covering both the dispatcher (`mimofan`)
/// and the legacy TUI (`mimofan`) binaries across our published
/// platform/arch matrix, plus a checksum sibling that must never be picked
/// as the primary binary.
fn mocked_release() -> Release {
    let json = r#"{
      "tag_name": "v0.8.8",
      "assets": [
        { "name": "mimofan-linux-x64",          "browser_download_url": "https://example.invalid/mimofan-linux-x64" },
        { "name": "mimofan-macos-x64",          "browser_download_url": "https://example.invalid/mimofan-macos-x64" },
        { "name": "mimofan-macos-arm64",        "browser_download_url": "https://example.invalid/mimofan-macos-arm64" },
        { "name": "mimofan-windows-x64.exe",    "browser_download_url": "https://example.invalid/mimofan-windows-x64.exe" },
        { "name": "mimofan-windows-x64.exe.sha256", "browser_download_url": "https://example.invalid/mimofan-windows-x64.exe.sha256" }
      ]
    }"#;
    serde_json::from_str(json).expect("mock release JSON")
}

#[test]
fn mocked_release_selects_dispatcher_asset_for_supported_platforms() {
    let release = mocked_release();
    let cases = [
        ("macos", "aarch64", "mimofan-macos-arm64"),
        ("macos", "x86_64", "mimofan-macos-x64"),
        ("linux", "x86_64", "mimofan-linux-x64"),
        ("windows", "x86_64", "mimofan-windows-x64.exe"),
    ];

    for (os, arch, expected) in cases {
        let stem = release_asset_stem_for(Path::new("/usr/local/bin/mimofan"), os, arch);
        let asset = select_platform_asset(&release, &stem)
            .unwrap_or_else(|| panic!("no asset for {os}/{arch} (stem {stem})"));
        assert_eq!(asset.name, expected, "{os}/{arch}");
    }
}

#[test]
fn mocked_release_selects_mimofan_asset() {
    let release = mocked_release();
    let stem = release_asset_stem_for(Path::new("/usr/local/bin/mimofan"), "macos", "aarch64");
    let asset = select_platform_asset(&release, &stem).expect("mimofan platform asset");
    assert_eq!(asset.name, "mimofan-macos-arm64");
}

#[test]
fn mirror_release_uses_base_url_and_platform_assets() {
    let release = release_from_mirror_base_url(
        "https://mirror.example/releases/v0.8.36/",
        "0.8.36",
        "linux",
        "x86_64",
    );

    assert_eq!(release.tag_name, "v0.8.36");
    assert_eq!(release.assets[0].name, CHECKSUM_MANIFEST_ASSET);
    assert_eq!(
        release.assets[0].browser_download_url,
        "https://mirror.example/releases/v0.8.36/mimofan-artifacts-sha256.txt"
    );

    let dispatcher =
        select_platform_asset(&release, "mimofan-linux-x64").expect("dispatcher asset");
    assert_eq!(
        dispatcher.browser_download_url,
        "https://mirror.example/releases/v0.8.36/mimofan-linux-x64"
    );
    // After the binary merge, only mimofan assets exist (no mimofan-tui)
    assert!(select_platform_asset(&release, "mimofan-tui-linux-x64").is_none());
}

#[test]
fn mirror_release_uses_windows_exe_asset_names() {
    let release = release_from_mirror_base_url(
        "https://mirror.example/releases/v0.8.36",
        "v0.8.36",
        "windows",
        "x86_64",
    );

    assert_eq!(release.tag_name, "v0.8.36");
    assert!(
        select_platform_asset(&release, "mimofan-windows-x64")
            .is_some_and(|asset| asset.name == "mimofan-windows-x64.exe")
    );
    // After the binary merge, only mimofan assets exist (no mimofan-tui)
    assert!(select_platform_asset(&release, "mimofan-tui-windows-x64").is_none());
}

#[test]
fn github_release_url_parser_extracts_tag() {
    let url = reqwest::Url::parse("https://github.com/XiaomingX/mimofan/releases/tag/v0.8.61")
        .expect("unexpected None/Err in test");

    assert_eq!(
        release_tag_from_github_release_url(&url).as_deref(),
        Some("v0.8.61")
    );
}

#[test]
fn github_release_download_fallback_uses_deterministic_asset_urls() {
    let release = release_from_github_download_tag("0.8.61", "macos", "aarch64");

    assert_eq!(release.tag_name, "v0.8.61");
    assert_eq!(
        release.assets[0].browser_download_url,
        "https://github.com/XiaomingX/mimofan/releases/download/v0.8.61/mimofan-artifacts-sha256.txt"
    );
    let dispatcher =
        select_platform_asset(&release, "mimofan-macos-arm64").expect("dispatcher asset");
    assert_eq!(
        dispatcher.browser_download_url,
        "https://github.com/XiaomingX/mimofan/releases/download/v0.8.61/mimofan-macos-arm64"
    );
    // After the binary merge, only mimofan assets exist (no mimofan-tui)
    assert!(select_platform_asset(&release, "mimofan-tui-macos-arm64").is_none());
}

#[test]
fn latest_stable_redirect_fallback_reads_tag_url() {
    let (url, request_rx, handle) = serve_http_once("200 OK", "text/html", b"<html></html>");
    let tag_url = url.replace("/release", "/XiaomingX/mimofan/releases/tag/v9.9.9");

    let tag = fetch_latest_stable_tag_from_redirect_url(&tag_url, None)
        .expect("tag should parse from final URL");

    assert_eq!(tag, "v9.9.9");
    let request = request_rx.recv().expect("captured request");
    assert!(
        request.starts_with("GET /XiaomingX/mimofan/releases/tag/v9.9.9 "),
        "got {request:?}"
    );
    handle.join().expect("test server thread");
}

#[test]
fn github_release_html_parser_skips_empty_first_marker() {
    let body = r#"
            <a href="/XiaomingX/mimofan/releases/tag/?expanded=true">generic</a>
            <a href="/XiaomingX/mimofan/releases/tag/v9.9.9">latest</a>
        "#;

    assert_eq!(
        release_tag_from_github_release_html(body).as_deref(),
        Some("v9.9.9")
    );
}

#[test]
fn beta_release_detection_requires_beta_tag() {
    let rc_prerelease = Release {
        tag_name: "v0.9.0-rc.1".to_string(),
        prerelease: true,
        assets: vec![],
    };
    let beta_tag = Release {
        tag_name: "v0.9.0-beta.1".to_string(),
        prerelease: false,
        assets: vec![],
    };
    let stable = Release {
        tag_name: "v0.9.0".to_string(),
        prerelease: false,
        assets: vec![],
    };

    assert!(!is_beta_tag(&rc_prerelease.tag_name));
    assert!(is_beta_tag(&beta_tag.tag_name));
    assert!(!is_beta_tag(&stable.tag_name));
}

#[test]
fn update_fallback_hint_points_to_asset_mirrors() {
    let hint = update_network_fallback_hint();

    assert!(
        hint.contains(mimofan_release::RELEASE_BASE_URL_ENV),
        "{hint}"
    );
    assert!(hint.contains(mimofan_release::UPDATE_VERSION_ENV), "{hint}");
}

fn serve_http_responses(
    responses: Vec<(&'static str, &'static str, &'static [u8])>,
) -> (String, mpsc::Receiver<String>, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
    let addr = listener.local_addr().expect("test server addr");
    let (request_tx, request_rx) = mpsc::channel();

    let handle = thread::spawn(move || {
        for (status, content_type, body) in responses {
            let (mut stream, _) = listener.accept().expect("accept test request");
            let mut buf = [0_u8; 4096];
            let n = stream.read(&mut buf).expect("read test request");
            request_tx
                .send(String::from_utf8_lossy(&buf[..n]).to_string())
                .expect("send captured request");

            write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .expect("write test response headers");
            stream.write_all(body).expect("write test response body");
        }
    });

    (format!("http://{addr}/release"), request_rx, handle)
}

fn serve_http_once(
    status: &'static str,
    content_type: &'static str,
    body: &'static [u8],
) -> (String, mpsc::Receiver<String>, thread::JoinHandle<()>) {
    serve_http_responses(vec![(status, content_type, body)])
}

#[test]
fn validate_and_build_proxy_accepts_supported_proxy_urls() {
    validate_and_build_proxy("http://localhost:7897").expect("http proxy");
    validate_and_build_proxy("https://proxy.example.com:8080").expect("https proxy");
    validate_and_build_proxy("socks5://127.0.0.1:1080").expect("socks proxy");
}

#[test]
fn validate_and_build_proxy_rejects_malformed_urls() {
    let err = validate_and_build_proxy("not a valid url").expect_err("malformed URL");
    assert!(err.to_string().contains("invalid proxy URL"));
}

#[test]
fn fetch_latest_release_from_url_reads_mocked_release_json() {
    let body = br#"{
      "tag_name": "v9.9.9",
      "assets": [
        { "name": "mimofan-linux-x64", "browser_download_url": "http://example.invalid/mimofan-linux-x64" },
        { "name": "mimofan-artifacts-sha256.txt", "browser_download_url": "http://example.invalid/mimofan-artifacts-sha256.txt" }
      ]
    }"#;
    let (url, request_rx, handle) = serve_http_once("200 OK", "application/json", body);
    let release = fetch_latest_release_from_url(&url, None).expect("release JSON should parse");

    assert_eq!(release.tag_name, "v9.9.9");
    assert_eq!(release.assets.len(), 2);

    let request = request_rx.recv().expect("captured request");
    let request_lower = request.to_ascii_lowercase();
    assert!(request.starts_with("GET /release "), "got {request:?}");
    assert!(
        request_lower.contains("accept: application/vnd.github+json"),
        "got {request:?}"
    );
    assert!(
        request_lower.contains("user-agent: mimofan-updater"),
        "got {request:?}"
    );
    handle.join().expect("test server thread");
}

#[test]
fn fetch_latest_release_from_url_retries_transient_gateway_error() {
    let body = br#"{
      "tag_name": "v9.9.9",
      "assets": [
        { "name": "mimofan-linux-x64", "browser_download_url": "http://example.invalid/mimofan-linux-x64" }
      ]
    }"#;
    let (url, request_rx, handle) = serve_http_responses(vec![
        ("504 Gateway Timeout", "text/plain", b"gateway timeout"),
        ("200 OK", "application/json", body),
    ]);
    let release =
        fetch_latest_release_from_url(&url, None).expect("release JSON should parse after retry");

    assert_eq!(release.tag_name, "v9.9.9");
    let first = request_rx.recv().expect("first request");
    let second = request_rx.recv().expect("second request");
    assert!(first.starts_with("GET /release "), "got {first:?}");
    assert!(second.starts_with("GET /release "), "got {second:?}");
    handle.join().expect("test server thread");
}

#[test]
fn fetch_latest_release_from_url_reports_http_errors() {
    let (url, _request_rx, handle) = serve_http_responses(vec![
        ("500 Internal Server Error", "text/plain", b"server broke"),
        ("500 Internal Server Error", "text/plain", b"server broke"),
        ("500 Internal Server Error", "text/plain", b"server broke"),
    ]);
    let err = fetch_latest_release_from_url(&url, None).expect_err("HTTP 500 should fail");

    assert!(
        err.to_string().contains("HTTP 500"),
        "unexpected error: {err:#}"
    );
    handle.join().expect("test server thread");
}

#[test]
fn fetch_latest_beta_release_from_url_selects_first_beta_release() {
    let body = br#"[
      { "tag_name": "v0.9.0", "prerelease": false, "assets": [] },
      { "tag_name": "v0.9.0-rc.1", "prerelease": true, "assets": [] },
      { "tag_name": "v0.9.0-beta.2", "prerelease": true, "assets": [
        { "name": "mimofan-linux-x64", "browser_download_url": "http://example.invalid/mimofan-linux-x64" }
      ] },
      { "tag_name": "v0.9.0-beta.1", "prerelease": true, "assets": [] }
    ]"#;
    let (url, request_rx, handle) = serve_http_once("200 OK", "application/json", body);
    let release =
        fetch_latest_beta_release_from_url(&url, None).expect("beta release JSON should parse");

    assert_eq!(release.tag_name, "v0.9.0-beta.2");
    assert!(release.prerelease);

    let request = request_rx.recv().expect("captured request");
    let request_lower = request.to_ascii_lowercase();
    assert!(request.starts_with("GET /release "), "got {request:?}");
    assert!(
        request_lower.contains("accept: application/vnd.github+json"),
        "got {request:?}"
    );
    handle.join().expect("test server thread");
}

#[test]
fn fetch_latest_beta_release_from_url_reports_missing_beta() {
    let body = br#"[
      { "tag_name": "v0.9.0", "prerelease": false, "assets": [] }
    ]"#;
    let (url, _request_rx, handle) = serve_http_once("200 OK", "application/json", body);
    let err = fetch_latest_beta_release_from_url(&url, None).expect_err("missing beta should fail");

    assert!(
        err.to_string().contains("no beta release found"),
        "unexpected error: {err:#}"
    );
    handle.join().expect("test server thread");
}

#[test]
fn download_url_retries_transient_gateway_error() {
    let (url, request_rx, handle) = serve_http_responses(vec![
        ("503 Service Unavailable", "text/plain", b"try again"),
        ("200 OK", "application/octet-stream", b"\0binary bytes"),
    ]);
    let bytes = download_url(&url, None).expect("binary download should retry and succeed");

    assert_eq!(bytes, b"\0binary bytes");
    let first = request_rx.recv().expect("first request");
    let second = request_rx.recv().expect("second request");
    assert!(first.starts_with("GET /release "), "got {first:?}");
    assert!(second.starts_with("GET /release "), "got {second:?}");
    handle.join().expect("test server thread");
}

#[test]
fn download_url_reads_binary_body_with_updater_user_agent() {
    let (url, request_rx, handle) =
        serve_http_once("200 OK", "application/octet-stream", b"\0binary bytes");
    let bytes = download_url(&url, None).expect("binary download should succeed");

    assert_eq!(bytes, b"\0binary bytes");

    let request = request_rx.recv().expect("captured request");
    let request_lower = request.to_ascii_lowercase();
    assert!(request.starts_with("GET /release "), "got {request:?}");
    assert!(
        request_lower.contains("user-agent: mimofan-updater"),
        "got {request:?}"
    );
    handle.join().expect("test server thread");
}
