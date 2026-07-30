#!/usr/bin/env bash
# Mimofan Benchmark Runner
# 一键运行所有基准测试

set -euo pipefail

# 配置
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# 颜色输出
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo -e "${YELLOW}===================================${NC}"
echo -e "${YELLOW}  Mimofan Benchmark Suite${NC}"
echo -e "${YELLOW}===================================${NC}"

# 检查环境变量
if [ -z "${MIMOFAN_TEST_API_KEY:-}" ]; then
    echo -e "${RED}Error: MIMOFAN_TEST_API_KEY not set${NC}"
    echo ""
    echo "Please set the following environment variables:"
    echo "  export MIMOFAN_TEST_API_KEY=your_api_key"
    echo "  export MIMOFAN_TEST_BASE_URL=your_base_url"
    echo "  export MIMOFAN_TEST_MODEL=your_model"
    echo ""
    echo "Example:"
    echo "  export MIMOFAN_TEST_API_KEY=sk-sxkdoorynshjsdzgy0h45gtwfd19fqmaronc48x5pyh3klku"
    echo "  export MIMOFAN_TEST_BASE_URL=https://api.xiaomimimo.com/anthropic"
    echo "  export MIMOFAN_TEST_MODEL=mimo-v2.5"
    exit 1
fi

# 运行基准测试（包含所有 exec 和 CLI 命令测试）
echo -e "\n${YELLOW}Running Benchmark...${NC}"
if "${SCRIPT_DIR}/tui_benchmark.sh"; then
    echo -e "${GREEN}Benchmark completed successfully${NC}"
else
    echo -e "${RED}Benchmark had failures${NC}"
fi

# 计算指标
echo -e "\n${YELLOW}Calculating Metrics...${NC}"
"${SCRIPT_DIR}/metrics.sh"

echo -e "\n${YELLOW}===================================${NC}"
echo -e "${YELLOW}  All Benchmarks Complete${NC}"
echo -e "${YELLOW}===================================${NC}"
echo ""
echo "Results saved to: ${SCRIPT_DIR}/results/"
echo ""
echo "View results:"
echo "  ls -la ${SCRIPT_DIR}/results/"
echo ""
echo "View summary:"
echo "  cat ${SCRIPT_DIR}/results/*_summary_*.txt"
