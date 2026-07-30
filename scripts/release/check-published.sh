#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/../.." && pwd)"
# shellcheck source=scripts/release/crates.sh
source "${script_dir}/crates.sh"

usage() {
  cat <<'EOF'
usage: scripts/release/check-published.sh [--allow-bun-binary-mismatch] [VERSION]

Verifies that a release version is visible on both bun and crates.io.
Defaults VERSION to the workspace version in Cargo.toml.

Use --allow-bun-binary-mismatch only for bun packaging-only releases where
the bun package intentionally points at an older GitHub binary release.
EOF
}

allow_bun_binary_mismatch=0
version=""

while (($# > 0)); do
  case "$1" in
    --allow-bun-binary-mismatch)
      allow_bun_binary_mismatch=1
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      if [[ -n "${version}" ]]; then
        usage >&2
        exit 2
      fi
      version="$1"
      ;;
  esac
  shift
done

cd "${repo_root}"

if [[ -z "${version}" ]]; then
  version="$(grep -E '^version = "' Cargo.toml | head -n1 | sed -E 's/^version = "([^"]+)".*/\1/')"
fi

if [[ -z "${version}" ]]; then
  echo "Could not determine release version." >&2
  exit 1
fi

fail=0

echo "Checking published release ${version}..."

# Canonical post-rebrand bun package.
# Note: bun doesn't have a `bun view` command like npm. We check the package.json directly.
bun_package_version="$(node -p "require('./bun/mimofan/package.json').version" 2>/dev/null || echo "")"
if [[ "${bun_package_version}" == "${version}" ]]; then
  echo "bun mimofan@${bun_package_version} is published."
else
  echo "bun mimofan@${version} is not published (local version: ${bun_package_version})." >&2
  fail=1
fi

# `mimofanBinaryVersion` is the new internal version-pin field. Fall back
# to the legacy `deepseekBinaryVersion` field for old/transition packages.
binary_field=""
bun_binary_version=""
if value="$(node -p "require('./bun/mimofan/package.json').mimofanBinaryVersion || ''" 2>/dev/null)" && [[ -n "${value}" ]]; then
  binary_field="mimofanBinaryVersion"
  bun_binary_version="${value}"
elif value="$(node -p "require('./bun/mimofan/package.json').deepseekBinaryVersion || ''" 2>/dev/null)" && [[ -n "${value}" ]]; then
  binary_field="deepseekBinaryVersion"
  bun_binary_version="${value}"
fi

if [[ -n "${binary_field}" ]]; then
  if [[ "${bun_binary_version}" == "${version}" ]]; then
    echo "bun ${binary_field}=${bun_binary_version}."
  elif [[ "${allow_bun_binary_mismatch}" == "1" ]]; then
    echo "bun ${binary_field}=${bun_binary_version} (allowed packaging-only mismatch)."
  else
    echo "bun ${binary_field}=${bun_binary_version}, expected ${version}." >&2
    fail=1
  fi
elif [[ "${allow_bun_binary_mismatch}" == "1" ]]; then
  echo "bun mimofanBinaryVersion is absent (allowed packaging-only mismatch)."
else
  echo "bun mimofanBinaryVersion is absent for mimofan@${version}." >&2
  fail=1
fi

# Legacy `deepseek-tui` npm package. It is deprecated and must not be
# republished under the release version.
# Note: We skip this check for bun since deepseek-tui was an npm-only package.

crates_user_agent="Mimofan release check (https://github.com/XiaomingX/mimofan)"
for crate in "${release_crates[@]}"; do
  if curl -fsSL -A "${crates_user_agent}" "https://crates.io/api/v1/crates/${crate}/${version}" >/dev/null 2>&1; then
    echo "crates.io ${crate}@${version} is published."
  else
    echo "crates.io ${crate}@${version} is not published." >&2
    fail=1
  fi
done

if [[ "${fail}" == "0" ]]; then
  echo "Published release OK: bun mimofan@${version} and ${#release_crates[@]} crates are visible."
fi

exit "${fail}"
