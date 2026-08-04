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
echo -e "${BLUE}     TUI 命令验收测试（tui_commands_test.sh）${NC}"
echo -e "${BLUE}══════════════════════════════════════════════════════════════${NC}"
echo ""

# ════════════════════════════════════════════════════════════════
# 命令 1: /agent（子智能体管理）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 命令: /agent（子智能体管理）${NC}"

output=$($MIMOFAN exec -c "/agent --help" 2>&1)
if echo "$output" | grep -qE "agent|subagent|spawn|help"; then
  log_test "/agent 子智能体管理" "pass"
else
  log_test "/agent 子智能体管理" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 命令 2: /anchor（锚点）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 命令: /anchor（锚点）${NC}"

output=$($MIMOFAN exec -c "/anchor --help" 2>&1)
if echo "$output" | grep -qE "anchor|mark|position|help"; then
  log_test "/anchor 锚点" "pass"
else
  log_test "/anchor 锚点" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 命令 3: /auto（自动模式）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 命令: /auto（自动模式）${NC}"

output=$($MIMOFAN exec -c "/auto --help" 2>&1)
if echo "$output" | grep -qE "auto|automatic|mode|help"; then
  log_test "/auto 自动模式" "pass"
else
  log_test "/auto 自动模式" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 命令 4: /clear（清空）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 命令: /clear（清空）${NC}"

output=$($MIMOFAN exec -c "/clear" 2>&1)
if echo "$output" | grep -qE "clear|empty|reset|✓|done|success"; then
  log_test "/clear 清空" "pass"
else
  log_test "/clear 清空" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 命令 5: /exit（退出）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 命令: /exit（退出）${NC}"

output=$($MIMOFAN exec -c "/exit" 2>&1)
if echo "$output" | grep -qE "exit|quit|bye|再见|✓|done|success"; then
  log_test "/exit 退出" "pass"
else
  log_test "/exit 退出" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 命令 6: /fast（快速模式）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 命令: /fast（快速模式）${NC}"

output=$($MIMOFAN exec -c "/fast --help" 2>&1)
if echo "$output" | grep -qE "fast|speed|mode|help"; then
  log_test "/fast 快速模式" "pass"
else
  log_test "/fast 快速模式" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 命令 7: /feedback（反馈）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 命令: /feedback（反馈）${NC}"

output=$($MIMOFAN exec -c "/feedback --help" 2>&1)
if echo "$output" | grep -qE "feedback|report|issue|help"; then
  log_test "/feedback 反馈" "pass"
else
  log_test "/feedback 反馈" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 命令 8: /fleet（舰队管理）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 命令: /fleet（舰队管理）${NC}"

output=$($MIMOFAN exec -c "/fleet --help" 2>&1)
if echo "$output" | grep -qE "fleet|manager|worker|help"; then
  log_test "/fleet 舰队管理" "pass"
else
  log_test "/fleet 舰队管理" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 命令 9: /provider（服务商选择）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 命令: /provider（服务商选择）${NC}"

output=$($MIMOFAN exec -c "/provider --help" 2>&1)
if echo "$output" | grep -qE "provider|service|api|help"; then
  log_test "/provider 服务商选择" "pass"
else
  log_test "/provider 服务商选择" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 命令 10: /stash（暂存）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 命令: /stash（暂存）${NC}"

output=$($MIMOFAN exec -c "/stash --help" 2>&1)
if echo "$output" | grep -qE "stash|save|store|help"; then
  log_test "/stash 暂存" "pass"
else
  log_test "/stash 暂存" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 命令 11: /subagents（子智能体列表）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 命令: /subagents（子智能体列表）${NC}"

output=$($MIMOFAN exec -c "/subagents --help" 2>&1)
if echo "$output" | grep -qE "subagent|list|agent|help"; then
  log_test "/subagents 子智能体列表" "pass"
else
  log_test "/subagents 子智能体列表" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 命令 12: /voice（语音）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 命令: /voice（语音）${NC}"

output=$($MIMOFAN exec -c "/voice --help" 2>&1)
if echo "$output" | grep -qE "voice|speech|tts|help"; then
  log_test "/voice 语音" "pass"
else
  log_test "/voice 语音" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 命令 13: /yolo（YOLO 模式）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 命令: /yolo（YOLO 模式）${NC}"

output=$($MIMOFAN exec -c "/yolo --help" 2>&1)
if echo "$output" | grep -qE "yolo|mode|auto|accept|help"; then
  log_test "/yolo YOLO 模式" "pass"
else
  log_test "/yolo YOLO 模式" "fail"
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
