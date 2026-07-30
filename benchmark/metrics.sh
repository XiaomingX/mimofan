#!/usr/bin/env bash
# Mimofan Metrics Calculator
# 计算基准测试的评估指标

set -euo pipefail

# 配置
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RESULTS_DIR="${1:-${SCRIPT_DIR}/results}"

# 颜色输出
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

# 检查结果目录
if [ ! -d "$RESULTS_DIR" ]; then
    echo -e "${RED}Error: Results directory not found: ${RESULTS_DIR}${NC}"
    exit 1
fi

# 查找最新的结果文件
LATEST_CLI=$(ls -t "${RESULTS_DIR}"/cli_benchmark_*.json 2>/dev/null | head -1)
LATEST_TUI=$(ls -t "${RESULTS_DIR}"/tui_benchmark_*.json 2>/dev/null | head -1)

if [ -z "$LATEST_CLI" ] && [ -z "$LATEST_TUI" ]; then
    echo -e "${RED}Error: No benchmark results found in ${RESULTS_DIR}${NC}"
    exit 1
fi

# 函数：计算 JSON 文件的指标
calculate_metrics() {
    local json_file="$1"
    local binary_type="$2"

    echo -e "\n${BLUE}===================================${NC}"
    echo -e "${BLUE}  ${binary_type} Metrics${NC}"
    echo -e "${BLUE}===================================${NC}"

    # 使用 python 解析 JSON（如果可用）
    if command -v python3 &> /dev/null; then
        python3 << EOF
import json
import sys

with open('${json_file}', 'r') as f:
    data = json.load(f)

total = len(data)
passed = sum(1 for r in data if r['status'] == 'PASS')
failed = total - passed

success_rate = (passed / total * 100) if total > 0 else 0

latencies = [r['latency_seconds'] for r in data]
latencies.sort()

avg_latency = sum(latencies) / len(latencies) if latencies else 0
p50_index = len(latencies) // 2
p95_index = int(len(latencies) * 0.95)
latency_p50 = latencies[p50_index] if latencies else 0
latency_p95 = latencies[p95_index] if latencies else 0

output_lengths = [r['output_length'] for r in data]
avg_output = sum(output_lengths) / len(output_lengths) if output_lengths else 0

print(f"Total Tests: {total}")
print(f"Passed: {passed}")
print(f"Failed: {failed}")
print(f"Success Rate: {success_rate:.2f}%")
print()
print("Latency Statistics:")
print(f"  Average: {avg_latency:.2f}s")
print(f"  P50: {latency_p50:.2f}s")
print(f"  P95: {latency_p95:.2f}s")
print()
print(f"Average Output Length: {avg_output:.0f} chars")
print()

# 评分
score = 0
if success_rate >= 90:
    score += 40
elif success_rate >= 80:
    score += 35
elif success_rate >= 70:
    score += 30
elif success_rate >= 60:
    score += 25
else:
    score += 20

if latency_p50 < 2:
    score += 30
elif latency_p50 < 5:
    score += 25
elif latency_p50 < 10:
    score += 20
elif latency_p50 < 20:
    score += 15
else:
    score += 10

if avg_output > 100:
    score += 30
elif avg_output > 50:
    score += 25
elif avg_output > 20:
    score += 20
else:
    score += 15

grade = 'S' if score >= 90 else 'A' if score >= 80 else 'B' if score >= 70 else 'C' if score >= 60 else 'D'

print(f"Overall Score: {score}/100")
print(f"Grade: {grade}")
EOF
    else
        echo -e "${YELLOW}Python3 not available, using basic calculation${NC}"

        # 基本计算
        local total=$(grep -c "test_name" "$json_file" || echo "0")
        local passed=$(grep -c '"status": "PASS"' "$json_file" || echo "0")
        local failed=$(grep -c '"status": "FAIL"' "$json_file" || echo "0")

        echo "Total Tests: $total"
        echo "Passed: $passed"
        echo "Failed: $failed"

        if [ "$total" -gt 0 ]; then
            local success_rate=$(echo "scale=2; $passed * 100 / $total" | bc)
            echo "Success Rate: ${success_rate}%"
        fi
    fi
}

# 函数：生成对比报告
generate_comparison() {
    echo -e "\n${BLUE}===================================${NC}"
    echo -e "${BLUE}  Comparison Report${NC}"
    echo -e "${BLUE}===================================${NC}"

    if [ -n "$LATEST_CLI" ] && [ -n "$LATEST_TUI" ]; then
        echo "CLI Results: $LATEST_CLI"
        echo "TUI Results: $LATEST_TUI"
        echo ""
        echo "Note: Direct comparison may not be meaningful as"
        echo "CLI and TUI have different capability scopes."
    elif [ -n "$LATEST_CLI" ]; then
        echo "Only CLI results available: $LATEST_CLI"
    else
        echo "Only TUI results available: $LATEST_TUI"
    fi
}

# 函数：列出所有历史结果
list_history() {
    echo -e "\n${BLUE}===================================${NC}"
    echo -e "${BLUE}  Benchmark History${NC}"
    echo -e "${BLUE}===================================${NC}"

    echo "CLI Benchmarks:"
    ls -lt "${RESULTS_DIR}"/cli_benchmark_*.json 2>/dev/null | head -5 || echo "  No CLI benchmarks found"
    echo ""

    echo "TUI Benchmarks:"
    ls -lt "${RESULTS_DIR}"/tui_benchmark_*.json 2>/dev/null | head -5 || echo "  No TUI benchmarks found"
    echo ""

    echo "Summaries:"
    ls -lt "${RESULTS_DIR}"/*_summary_*.txt 2>/dev/null | head -10 || echo "  No summaries found"
}

# 主函数
main() {
    echo -e "${YELLOW}===================================${NC}"
    echo -e "${YELLOW}  Mimofan Metrics Calculator${NC}"
    echo -e "${YELLOW}===================================${NC}"

    # 计算 CLI 指标
    if [ -n "$LATEST_CLI" ]; then
        calculate_metrics "$LATEST_CLI" "CLI"
    fi

    # 计算 TUI 指标
    if [ -n "$LATEST_TUI" ]; then
        calculate_metrics "$LATEST_TUI" "TUI"
    fi

    # 生成对比报告
    generate_comparison

    # 列出历史结果
    list_history
}

main "$@"
