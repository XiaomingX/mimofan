#!/usr/bin/env bash
set -euo pipefail
# Mimofan Unix installer
# Copies mimofan and mimofan-tui to ~/.local/bin (or $PREFIX/bin)

PREFIX="${PREFIX:-$HOME/.local}"
BIN_DIR="${PREFIX}/bin"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

version_code() {
    local version="$1"
    local major minor patch
    IFS=. read -r major minor patch <<< "$version"
    printf '%d%03d%03d\n' "${major:-0}" "${minor:-0}" "${patch:-0}"
}

detect_host_glibc() {
    local out
    if out="$(getconf GNU_LIBC_VERSION 2>/dev/null)"; then
        printf '%s\n' "$out" | awk '{print $NF; exit}'
        return 0
    fi
    if out="$(ldd --version 2>&1 | head -n 1)"; then
        printf '%s\n' "$out" | grep -Eo '[0-9]+\.[0-9]+(\.[0-9]+)?' | head -n 1
        return 0
    fi
    return 1
}

required_glibc_for_binary() {
    local bin="$1"
    local versions
    versions="$(grep -aoE 'GLIBC_[0-9]+\.[0-9]+(\.[0-9]+)?' "$bin" 2>/dev/null | sed 's/^GLIBC_//' || true)"
    if [[ -z "$versions" ]]; then
        return 1
    fi
    printf '%s\n' "$versions" | awk -F. '
        {
            patch = ($3 == "" ? 0 : $3)
            code = ($1 * 1000000) + ($2 * 1000) + patch
            if (code > best) {
                best = code
                value = $0
            }
        }
        END {
            if (value != "") print value
        }
    '
}

preflight_glibc() {
    local bin="$1"
    if [[ "$(uname -s)" != "Linux" ]]; then
        return 0
    fi
    if [[ "${MIMOFAN_SKIP_GLIBC_CHECK:-}" == "1" ]]; then
        return 0
    fi

    local required
    if ! required="$(required_glibc_for_binary "$bin")" || [[ -z "$required" ]]; then
        return 0
    fi

    local host
    if ! host="$(detect_host_glibc)" || [[ -z "$host" ]]; then
        echo "ERROR: $(basename "$bin") requires GLIBC_$required, but no GNU libc was detected." >&2
        echo "Build from source instead: cargo install mimofan --locked" >&2
        echo "Set MIMOFAN_SKIP_GLIBC_CHECK=1 to bypass this check at your own risk." >&2
        return 1
    fi

    if [[ "$(version_code "$host")" -lt "$(version_code "$required")" ]]; then
        echo "ERROR: $(basename "$bin") requires GLIBC_$required, but this system has glibc $host." >&2
        echo "Ubuntu 22.04 ships glibc 2.35 and cannot run assets built against Ubuntu 24.04/glibc 2.39." >&2
        echo "Build from source instead: cargo install mimofan --locked" >&2
        echo "Release follow-up: build Linux GNU assets against an older glibc baseline or add a musl/static asset." >&2
        echo "Set MIMOFAN_SKIP_GLIBC_CHECK=1 to bypass this check at your own risk." >&2
        return 1
    fi
}

mkdir -p "$BIN_DIR"

echo "Installing mimofan to $BIN_DIR ..."

# 二进制合并后发布产物只有 mimofan 一个可执行文件；mimofan-tui 仅在旧的
# 双二进制包里存在。此前这里把两者都当作必需，缺 mimofan-tui 就 exit 1，
# 导致当前发布的 bundle 根本装不上。现在：mimofan 必需，mimofan-tui 可选。
installed=0

src="$SCRIPT_DIR/mimofan"
if [[ ! -f "$src" ]]; then
    echo "ERROR: $src not found in archive"
    exit 1
fi
preflight_glibc "$src"
cp "$src" "$BIN_DIR/mimofan"
chmod +x "$BIN_DIR/mimofan"
echo "  $BIN_DIR/mimofan"
installed=$((installed + 1))

# 旧版双二进制包的兼容路径：存在才装，不存在不报错。
src="$SCRIPT_DIR/mimofan-tui"
if [[ -f "$src" ]]; then
    preflight_glibc "$src"
    cp "$src" "$BIN_DIR/mimofan-tui"
    chmod +x "$BIN_DIR/mimofan-tui"
    echo "  $BIN_DIR/mimofan-tui"
    installed=$((installed + 1))
fi

echo ""
if [[ "$installed" -eq 1 ]]; then
    echo "Done. mimofan installed to $BIN_DIR."
else
    echo "Done. $installed binaries installed to $BIN_DIR."
fi

# Check if BIN_DIR is on PATH
if [[ ":$PATH:" != *":$BIN_DIR:"* ]]; then
    echo ""
    echo "Add $BIN_DIR to your PATH:"
    echo ""
    SHELL_NAME="$(basename "${SHELL:-$SHELL}")"
    case "$SHELL_NAME" in
        zsh)  RC="$HOME/.zshrc" ;;
        bash) RC="$HOME/.bashrc" ;;
        fish) RC="$HOME/.config/fish/config.fish" ;;
        *)    RC="your shell profile" ;;
    esac
    echo "  echo 'export PATH=\"$BIN_DIR:\$PATH\"' >> $RC"
    echo "  source $RC"
fi

echo ""
echo "Then run: mimofan"
