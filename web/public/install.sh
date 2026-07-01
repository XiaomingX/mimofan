#!/bin/sh
set -eu

repo="XiaomingX/mimo-tui"
version="${MIMOFAN_VERSION:-latest}"
release_base="${MIMOFAN_RELEASE_BASE_URL:-${MIMOFAN_RELEASE_BASE_URL:-${MIMOFAN_RELEASE_BASE_URL:-}}}"

usage() {
  cat <<'USAGE'
Mimofan installer for macOS and Linux.

Usage:
  curl -fsSL https://mimofan.net/install.sh | sh

Environment:
  MIMOFAN_INSTALL_DIR    Install directory. Default: $HOME/.local/bin
  MIMOFAN_VERSION        Release tag to install, for example v0.8.64. Default: latest
  MIMOFAN_RELEASE_BASE_URL
                           Custom release asset base URL ending in /download
  MIMOFAN_SKIP_GLIBC_CHECK=1
                           Skip Linux arm64/riscv64 glibc compatibility preflight

Examples:
  curl -fsSL https://mimofan.net/install.sh | MIMOFAN_INSTALL_DIR=/usr/local/bin sh
  curl -fsSL https://mimofan.net/install.sh | MIMOFAN_VERSION=v0.8.64 sh
USAGE
}

case "${1:-}" in
  -h|--help)
    usage
    exit 0
    ;;
esac

say() {
  printf '%s\n' "$*"
}

fail() {
  printf 'mimofan install: %s\n' "$*" >&2
  exit 1
}

if [ -n "${MIMOFAN_INSTALL_DIR:-}" ]; then
  install_dir="$MIMOFAN_INSTALL_DIR"
else
  [ -n "${HOME:-}" ] || fail "HOME is not set; set MIMOFAN_INSTALL_DIR"
  install_dir="$HOME/.local/bin"
fi

need_cmd() {
  command -v "$1" >/dev/null 2>&1 || fail "missing required command: $1"
}

download() {
  url="$1"
  out="$2"
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$url" -o "$out"
  elif command -v wget >/dev/null 2>&1; then
    wget -q "$url" -O "$out"
  else
    fail "curl or wget is required"
  fi
}

sha256_file() {
  file="$1"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$file" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$file" | awk '{print $1}'
  else
    fail "sha256sum or shasum is required to verify downloads"
  fi
}

verify_asset() {
  asset="$1"
  file="$2"
  manifest="$3"
  expected="$(
    awk -v name="$asset" '
      {
        digest = tolower($1)
        file = $2
        sub(/^\*/, "", file)
        if (file == name && digest ~ /^[0-9a-f]{64}$/) {
          print digest
          exit
        }
      }
    ' "$manifest"
  )"
  [ -n "$expected" ] || fail "checksum not found for $asset"
  actual="$(sha256_file "$file" | tr '[:upper:]' '[:lower:]')"
  [ "$actual" = "$expected" ] || fail "checksum mismatch for $asset"
}

glibc_version() {
  if command -v getconf >/dev/null 2>&1; then
    getconf GNU_LIBC_VERSION 2>/dev/null | awk '{ print $NF; exit }'
    return
  fi
  if command -v ldd >/dev/null 2>&1; then
    ldd --version 2>/dev/null | awk 'NR == 1 {
      for (i = 1; i <= NF; i++) {
        if ($i ~ /^[0-9]+\.[0-9]+/) {
          print $i
          exit
        }
      }
    }'
  fi
}

version_at_least() {
  have="$1"
  need="$2"
  awk -v have="$have" -v need="$need" '
    BEGIN {
      split(have, h, ".")
      split(need, n, ".")
      for (i = 1; i <= 3; i++) {
        hv = h[i] + 0
        nv = n[i] + 0
        if (hv > nv) exit 0
        if (hv < nv) exit 1
      }
      exit 0
    }
  '
}

check_glibc() {
  case "$target" in
    linux-arm64|linux-riscv64) ;;
    *) return ;;
  esac

  [ "${MIMOFAN_SKIP_GLIBC_CHECK:-}" = "1" ] && return
  [ "${MIMOFAN_SKIP_GLIBC_CHECK:-}" = "1" ] && return
  [ "${MIMOFAN_SKIP_GLIBC_CHECK:-}" = "1" ] && return
  [ "${DEEPSEEK_SKIP_GLIBC_CHECK:-}" = "1" ] && return

  required="2.39"
  host="$(glibc_version || true)"
  if [ -z "$host" ] || ! version_at_least "$host" "$required"; then
    cat >&2 <<EOF
mimofan install: prebuilt Mimofan $target assets require glibc $required or newer.
This system reports glibc ${host:-unavailable}.

Linux x64 uses a static musl build. Linux arm64 and riscv64 release assets are
GNU libc builds from Ubuntu 24.04. Build from source with Cargo or set
MIMOFAN_SKIP_GLIBC_CHECK=1 to bypass this check at your own risk.
EOF
    exit 1
  fi
}

detect_platform() {
  os="$(uname -s)"
  arch="$(uname -m)"

  case "$os" in
    Darwin) platform="macos" ;;
    Linux) platform="linux" ;;
    *) fail "unsupported OS: $os. Use npm, Cargo, or the GitHub Releases page." ;;
  esac

  case "$arch" in
    x86_64|amd64) cpu="x64" ;;
    arm64|aarch64) cpu="arm64" ;;
    riscv64) cpu="riscv64" ;;
    *) fail "unsupported CPU architecture: $arch. Use Cargo or build from source." ;;
  esac

  if [ "$platform" = "macos" ] && [ "$cpu" = "riscv64" ]; then
    fail "macOS riscv64 is not a supported release target"
  fi

  printf '%s-%s' "$platform" "$cpu"
}

if [ -z "$release_base" ]; then
  if [ "$version" = "latest" ]; then
    release_base="https://github.com/$repo/releases/latest/download"
  else
    release_base="https://github.com/$repo/releases/download/$version"
  fi
fi

target="$(detect_platform)"
check_glibc
cli_asset="mimofan-$target"
tui_asset="mimofan-tui-$target"
manifest_asset="mimofan-artifacts-sha256.txt"

tmpdir="$(mktemp -d 2>/dev/null || mktemp -d -t mimofan-install)"
trap 'rm -rf "$tmpdir"' EXIT INT TERM

say "Installing Mimofan for $target"
say "Release assets: $release_base"
say "Install dir: $install_dir"

download "$release_base/$manifest_asset" "$tmpdir/$manifest_asset"
download "$release_base/$cli_asset" "$tmpdir/mimofan"
download "$release_base/$tui_asset" "$tmpdir/mimofan-tui"

verify_asset "$cli_asset" "$tmpdir/mimofan" "$tmpdir/$manifest_asset"
verify_asset "$tui_asset" "$tmpdir/mimofan-tui" "$tmpdir/$manifest_asset"
say "Checksums verified"

chmod 755 "$tmpdir/mimofan" "$tmpdir/mimofan-tui"
if command -v xattr >/dev/null 2>&1; then
  xattr -d com.apple.quarantine "$tmpdir/mimofan" "$tmpdir/mimofan-tui" 2>/dev/null || true
fi

sudo_cmd=""
if [ -d "$install_dir" ]; then
  if [ ! -w "$install_dir" ] ||
    { [ -e "$install_dir/mimofan" ] && [ ! -w "$install_dir/mimofan" ]; } ||
    { [ -e "$install_dir/mimofan-tui" ] && [ ! -w "$install_dir/mimofan-tui" ]; } ||
    { [ -e "$install_dir/codew" ] && [ ! -w "$install_dir/codew" ]; }; then
    need_cmd sudo
    sudo_cmd="sudo"
  fi
else
  if ! mkdir -p "$install_dir" 2>/dev/null; then
    need_cmd sudo
    sudo mkdir -p "$install_dir"
    sudo_cmd="sudo"
  fi
fi

stage_cli="$install_dir/.mimofan.$$"
stage_tui="$install_dir/.mimofan-tui.$$"
trap 'rm -rf "$tmpdir"; rm -f "$stage_cli" "$stage_tui" 2>/dev/null || true' EXIT INT TERM

$sudo_cmd cp "$tmpdir/mimofan" "$stage_cli"
$sudo_cmd cp "$tmpdir/mimofan-tui" "$stage_tui"
$sudo_cmd chmod 755 "$stage_cli" "$stage_tui"
$sudo_cmd mv "$stage_cli" "$install_dir/mimofan"
$sudo_cmd mv "$stage_tui" "$install_dir/mimofan-tui"

$sudo_cmd rm -f "$install_dir/codew"
if ! $sudo_cmd ln -s mimofan "$install_dir/codew"; then
  say "Installed binaries, but could not create $install_dir/codew alias"
fi

say "Installed:"
"$install_dir/mimofan" --version || true
"$install_dir/mimofan-tui" --version || true

case ":$PATH:" in
  *":$install_dir:"*) ;;
  *)
    say ""
    say "Add $install_dir to PATH to run mimofan from any terminal."
    ;;
esac

say ""
say "Run: mimofan"
