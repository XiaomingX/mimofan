#!/usr/bin/env bash
# benchmark/fleet_observability.sh
#
# fleet::observability 能力验收 benchmark
# 验收维度（MECE）：Topology / Metrics / Summary
#
# 运行: ./benchmark/fleet_observability.sh

set -euo pipefail
cd "$(dirname "$0")"

echo "════════════════════════════════════════════════════════════════"
echo "  fleet::observability 能力验收 benchmark"
echo "════════════════════════════════════════════════════════════════"
echo ""
echo "调用独立运行脚本..."
echo ""
./run_observability_bench.sh
