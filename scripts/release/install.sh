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
    if [[ "${MIMOFAN_SKIP_GLIBC_CHECK:-}" == "1" || "${MIMOFAN_SKIP_GLIBC_CHECK:-}" == "1" || "${MIMOFAN_SKIP_GLIBC_CHECK:-}" == "1" || "${DEEPSEEK_SKIP_GLIBC_CHECK:-}" == "1" ]]; then
        return 0
    fi

    local required
    if ! required="$(required_glibc_for_binary "$bin")" || [[ -z "$required" ]]; then
        return 0
    fi

    local host
    if ! host="$(detect_host_glibc)" || [[ -z "$host" ]]; then
        echo "ERROR: $(basename "$bin") requires GLIBC_$required, but no GNU libc was detected." >&2
        echo "Build from source instead: cargo install mimofan-cli --locked && cargo install mimofan-tui --locked" >&2
        echo "Set MIMOFAN_SKIP_GLIBC_CHECK=1 to bypass this check at your own risk." >&2
        return 1
    fi

    if [[ "$(version_code "$host")" -lt "$(version_code "$required")" ]]; then
        echo "ERROR: $(basename "$bin") requires GLIBC_$required, but this system has glibc $host." >&2
        echo "Ubuntu 22.04 ships glibc 2.35 and cannot run assets built against Ubuntu 24.04/glibc 2.39." >&2
        echo "Build from source instead: cargo install mimofan-cli --locked && cargo install mimofan-tui --locked" >&2
        echo "Release follow-up: build Linux GNU assets against an older glibc baseline or add a musl/static asset." >&2
        echo "Set MIMOFAN_SKIP_GLIBC_CHECK=1 to bypass this check at your own risk." >&2
        return 1
    fi
}

mkdir -p "$BIN_DIR"

echo "Installing mimofan to $BIN_DIR ..."

for bin in mimofan mimofan-tui; do
    src="$SCRIPT_DIR/$bin"
    dst="$BIN_DIR/$bin"
    if [[ ! -f "$src" ]]; then
        echo "ERROR: $src not found in archive"
        exit 1
    fi
    preflight_glibc "$src"
    cp "$src" "$dst"
    chmod +x "$dst"
    echo "  $dst"
done

echo ""
echo "Done. Both binaries installed to $BIN_DIR."

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
