#!/usr/bin/env bash
# benchmark/run_observability_bench.sh
#
# fleet::observability 能力验收 benchmark（独立模式，绕过 crate 预存编译错误）
# 验收维度（MECE）：Topology / Metrics / Summary
#
# 运行: ./benchmark/run_observability_bench.sh

set -euo pipefail
cd "$(dirname "$0")/.."

BENCH_DIR=$(mktemp -d)
trap 'rm -rf "$BENCH_DIR"' EXIT

TEST_FILE="$BENCH_DIR/test.rs"
cp crates/tui/tests/fleet_observability.rs "$TEST_FILE"

echo "════════════════════════════════════════════════════════════════"
echo "  fleet::observability 能力验收 benchmark（独立模式）"
echo "════════════════════════════════════════════════════════════════"
echo ""

cd "$BENCH_DIR"
cargo init --name observability_bench .
cp "$TEST_FILE" src/lib.rs

echo "编译并运行测试..."
echo ""
cargo test -- --nocapture 2>&1
STATUS=$?

echo ""
if [ "$STATUS" -eq 0 ]; then
    echo "════════════════════════════════════════════════════════════════"
    echo "  ✓ 全部测试通过"
    echo "════════════════════════════════════════════════════════════════"
else
    echo "════════════════════════════════════════════════════════════════"
    echo "  ✗ 存在失败的测试"
    echo "════════════════════════════════════════════════════════════════"
fi
exit $STATUS
