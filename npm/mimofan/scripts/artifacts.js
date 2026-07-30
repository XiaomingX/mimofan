const path = require("path");
const os = require("os");

const CHECKSUM_MANIFEST = "mimofan-artifacts-sha256.txt";

const ASSET_MATRIX = {
  linux: {
    x64: ["mimofan-linux-x64", "mimofan-tui-linux-x64"],
    arm64: ["mimofan-linux-arm64", "mimofan-tui-linux-arm64"],
    riscv64: ["mimofan-linux-riscv64", "mimofan-tui-linux-riscv64"],
  },
  darwin: {
    x64: ["mimofan-macos-x64", "mimofan-tui-macos-x64"],
    arm64: ["mimofan-macos-arm64", "mimofan-tui-macos-arm64"],
  },
  win32: {
    x64: ["mimofan-windows-x64.exe", "mimofan-tui-windows-x64.exe", "mimofan.bat"],
  },
};

// HarmonyPC (openharmony) is an x86_64 Linux-compatible environment; map it to
// the linux binary family so npm install succeeds without a separate build target.
const PLATFORM_ALIASES = {
  openharmony: "linux",
};

function detectBinaryNames() {
  const rawPlatform = os.platform();
  const platform = PLATFORM_ALIASES[rawPlatform] || rawPlatform;
  const arch = os.arch();
  const defaults = ASSET_MATRIX[platform];
  if (!defaults) {
    const supported = Object.keys(ASSET_MATRIX).map(p => `'${p}'`).join(', ');
    throw new Error(
      `Unsupported platform: ${rawPlatform}. Supported platforms: ${supported}.\n\n` +
      unsupportedBuildHint(),
    );
  }
  const pair = defaults[arch];
  if (!pair) {
    const supported = Object.keys(defaults).map(a => `'${a}'`).join(', ');
    throw new Error(
      `Unsupported architecture: ${arch} on platform ${platform}. ` +
      `Supported architectures: ${supported}.\n\n` +
      unsupportedBuildHint(),
    );
  }
  return {
    platform,
    arch,
    mimofan: pair[0],
    tui: pair[1],
  };
}

function unsupportedBuildHint() {
  return [
    "No prebuilt binary is available for this platform/architecture combo.",
    "You can still run mimofan by building from source with Cargo:",
    "",
    "  # Requires Rust 1.88+ (https://rustup.rs)",
    "  cargo install mimofan --locked",
    "",
    "Or build from a checkout:",
    "",
    "  git clone https://github.com/XiaomingX/mimofan.git",
    "  cd Mimofan",
    "  cargo install --path crates/tui --locked",
    "",
    "See https://github.com/XiaomingX/mimofan/blob/main/docs/INSTALL.md",
    "for cross-compilation, mirror, and Linux ARM64 specifics.",
  ].join("\n");
}

function executableName(base, platform) {
  return platform === "win32" ? `${base}.exe` : base;
}

function releaseBaseUrl(version, repo = "XiaomingX/mimofan") {
  // MIMOFAN_RELEASE_BASE_URL is the canonical override; DEEPSEEK_RELEASE_BASE_URL is a legacy alias.
  const override =
    process.env.MIMOFAN_RELEASE_BASE_URL ||
    process.env.DEEPSEEK_RELEASE_BASE_URL;
  if (override) {
    const trimmed = String(override).trim();
    return trimmed.endsWith("/") ? trimmed : `${trimmed}/`;
  }
  // When MIMOFAN_USE_CNB_MIRROR is set, use the CNB (China-friendly)
  // mirror that already builds and publishes binary release assets.
  if (process.env.MIMOFAN_USE_CNB_MIRROR) {
    return `https://cnb.cool/XiaomingX/mimofan/-/releases/v${version}/`;
  }
  return `https://github.com/${repo}/releases/download/v${version}/`;
}

function releaseAssetUrl(baseName, version, repo = "XiaomingX/mimofan") {
  return new URL(baseName, releaseBaseUrl(version, repo)).toString();
}

function checksumManifestUrl(version, repo = "XiaomingX/mimofan") {
  return releaseAssetUrl(CHECKSUM_MANIFEST, version, repo);
}

function releaseBinaryDirectory() {
  return path.join(__dirname, "..", "bin", "downloads");
}

function allAssetNames() {
  const names = [];
  for (const platformAssets of Object.values(ASSET_MATRIX)) {
    for (const assets of Object.values(platformAssets)) {
      names.push(...assets);
    }
  }
  return Array.from(new Set(names));
}

function allReleaseAssetNames() {
  return [...allAssetNames(), CHECKSUM_MANIFEST];
}

module.exports = {
  allAssetNames,
  allReleaseAssetNames,
  CHECKSUM_MANIFEST,
  checksumManifestUrl,
  detectBinaryNames,
  executableName,
  releaseAssetUrl,
  releaseBaseUrl,
  releaseBinaryDirectory,
};
