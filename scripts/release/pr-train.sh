#!/usr/bin/env bash
# PR Train Staging Helper for Mimofan Treework Workflows
#
# Validates local feature branches and batch merges on a temporary staging
# train before landing on main.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

cd "${WORKSPACE_ROOT}"

echo "========================================="
echo "  Mimofan PR-Train Pre-Landing Gate"
echo "========================================="

echo "1. Checking formatting..."
cargo fmt --all -- --check

echo "2. Running version and workspace integrity checks..."
"${SCRIPT_DIR}/check-versions.sh"

echo "3. Running workspace compilation check..."
cargo check --workspace

echo "========================================="
echo "  PR-Train Gate Status: PASS"
echo "========================================="
