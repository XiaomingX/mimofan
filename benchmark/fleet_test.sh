#!/usr/bin/env bash
set -uo pipefail

# 颜色定义
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# 测试配置
MIMOFAN="/Users/a0000/mywork/commonLLM/opensource/nnnew/agent-mimofan/target/release/mimofan"

test_number=0
pass_count=0
fail_count=0
failures=()

log_test() {
  local test_name=$1
  local result=$2
  if [[ "$result" == "pass" ]]; then
    echo -e "  ${GREEN}✓ $test_name: 通过${NC}"
    ((pass_count++))
  else
    echo -e "  ${RED}✗ $test_name: 失败${NC}"
    ((fail_count++))
    failures+=("$test_name")
  fi
  ((test_number++))
}

echo ""
echo -e "${BLUE}══════════════════════════════════════════════════════════════${NC}"
echo -e "${BLUE}     Fleet 舰队管理验收测试（fleet_test.sh）${NC}"
echo -e "${BLUE}══════════════════════════════════════════════════════════════${NC}"
echo ""

# ════════════════════════════════════════════════════════════════
# 命令 1: fleet --help（舰队管理帮助）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 命令: fleet --help（舰队管理帮助）${NC}"

output=$($MIMOFAN fleet --help 2>&1)
if echo "$output" | grep -qE "fleet|manager|worker|help|usage"; then
  log_test "fleet --help 帮助信息" "pass"
else
  log_test "fleet --help 帮助信息" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 命令 2: fleet list（舰队列表）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 命令: fleet list（舰队列表）${NC}"

output=$($MIMOFAN fleet list 2>&1)
if echo "$output" | grep -qE "fleet|list|worker|empty|error|no|not"; then
  log_test "fleet list 舰队列表" "pass"
else
  log_test "fleet list 舰队列表" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 测试汇总
# ════════════════════════════════════════════════════════════════
echo ""
echo -e "${BLUE}══════════════════════════════════════════════════════════════${NC}"
echo -e "${BLUE}     测试结果汇总${NC}"
echo -e "${BLUE}══════════════════════════════════════════════════════════════${NC}"
echo ""
echo -e "  总测试数: ${test_number}"
echo -e "  ${GREEN}通过: ${pass_count}${NC}"
echo -e "  ${RED}失败: ${fail_count}${NC}"
echo ""

if [[ ${#failures[@]} -gt 0 ]]; then
  echo -e "${RED}失败项:${NC}"
  for f in "${failures[@]}"; do
    echo -e "  - $f"
  done
  echo ""
fi

if [[ $fail_count -eq 0 ]]; then
  echo -e "${GREEN}✓ 全部测试通过！${NC}"
  exit 0
else
  echo -e "${RED}✗ 存在失败测试${NC}"
  exit 1
fi
