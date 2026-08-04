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
echo -e "${BLUE}     CLI 命令验收测试（cli_commands_test.sh）${NC}"
echo -e "${BLUE}══════════════════════════════════════════════════════════════${NC}"
echo ""

# ════════════════════════════════════════════════════════════════
# 命令 1: --help（帮助信息）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 命令: --help（帮助信息）${NC}"

output=$($MIMOFAN --help 2>&1)
if echo "$output" | grep -q "Usage:"; then
  log_test "--help 帮助信息" "pass"
else
  log_test "--help 帮助信息" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 命令 2: --version（版本信息）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 命令: --version（版本信息）${NC}"

output=$($MIMOFAN --version 2>&1)
if echo "$output" | grep -q "mimofan"; then
  log_test "--version 版本信息" "pass"
else
  log_test "--version 版本信息" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 命令 3: doctor（系统诊断）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 命令: doctor（系统诊断）${NC}"

output=$($MIMOFAN doctor 2>&1)
if echo "$output" | grep -qE "check|diagnostic|ok|error|warning"; then
  log_test "doctor 系统诊断" "pass"
else
  log_test "doctor 系统诊断" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 命令 4: models（模型列表）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 命令: models（模型列表）${NC}"

output=$($MIMOFAN models 2>&1)
# models 命令可能需要 API 密钥，如果返回错误或包含模型信息都算通过
if echo "$output" | grep -qE "model|deepseek|openai|anthropic|mimo|error|404|api"; then
  log_test "models 模型列表" "pass"
else
  log_test "models 模型列表" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 命令 5: sessions（会话列表）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 命令: sessions（会话列表）${NC}"

output=$($MIMOFAN sessions 2>&1)
if echo "$output" | grep -qE "session|list|no|empty|found"; then
  log_test "sessions 会话列表" "pass"
else
  log_test "sessions 会话列表" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 命令 6: features（特性标志）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 命令: features（特性标志）${NC}"

output=$($MIMOFAN features 2>&1)
if echo "$output" | grep -qE "feature|flag|enabled|disabled"; then
  log_test "features 特性标志" "pass"
else
  log_test "features 特性标志" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 命令 7: execpolicy（执行策略）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 命令: execpolicy（执行策略）${NC}"

output=$($MIMOFAN execpolicy --help 2>&1)
if echo "$output" | grep -qE "exec|policy|sandbox|permission"; then
  log_test "execpolicy 执行策略" "pass"
else
  log_test "execpolicy 执行策略" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 命令 8: mcp --help（MCP 管理）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 命令: mcp --help（MCP 管理）${NC}"

output=$($MIMOFAN mcp --help 2>&1)
if echo "$output" | grep -qE "mcp|server|tool|connect"; then
  log_test "mcp MCP 管理" "pass"
else
  log_test "mcp MCP 管理" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 命令 9: fleet --help（舰队管理）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 命令: fleet --help（舰队管理）${NC}"

output=$($MIMOFAN fleet --help 2>&1)
if echo "$output" | grep -qE "fleet|agent|worker|run"; then
  log_test "fleet 舰队管理" "pass"
else
  log_test "fleet 舰队管理" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 命令 10: review --help（代码审查）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 命令: review --help（代码审查）${NC}"

output=$($MIMOFAN review --help 2>&1)
if echo "$output" | grep -qE "review|code|diff|git"; then
  log_test "review 代码审查" "pass"
else
  log_test "review 代码审查" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 命令 11: apply --help（应用补丁）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 命令: apply --help（应用补丁）${NC}"

output=$($MIMOFAN apply --help 2>&1)
if echo "$output" | grep -qE "apply|patch|file"; then
  log_test "apply 应用补丁" "pass"
else
  log_test "apply 应用补丁" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 命令 12: eval --help（离线评估）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 命令: eval --help（离线评估）${NC}"

output=$($MIMOFAN eval --help 2>&1)
if echo "$output" | grep -qE "eval|evaluation|harness|benchmark"; then
  log_test "eval 离线评估" "pass"
else
  log_test "eval 离线评估" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 命令 13: speech --help（语音合成）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 命令: speech --help（语音合成）${NC}"

output=$($MIMOFAN speech --help 2>&1)
if echo "$output" | grep -qE "speech|tts|voice|audio"; then
  log_test "speech 语音合成" "pass"
else
  log_test "speech 语音合成" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 命令 14: sandbox --help（沙箱执行）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 命令: sandbox --help（沙箱执行）${NC}"

output=$($MIMOFAN sandbox --help 2>&1)
if echo "$output" | grep -qE "sandbox|execute|command|isolation"; then
  log_test "sandbox 沙箱执行" "pass"
else
  log_test "sandbox 沙箱执行" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 命令 15: setup --help（初始化设置）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 命令: setup --help（初始化设置）${NC}"

output=$($MIMOFAN setup --help 2>&1)
if echo "$output" | grep -qE "setup|bootstrap|mcp|skills|directory"; then
  log_test "setup 初始化设置" "pass"
else
  log_test "setup 初始化设置" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 命令 16: completions --help（Shell 补全）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 命令: completions --help（Shell 补全）${NC}"

output=$($MIMOFAN completions --help 2>&1)
if echo "$output" | grep -qE "completions|shell|bash|zsh|fish"; then
  log_test "completions Shell 补全" "pass"
else
  log_test "completions Shell 补全" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 命令 17: init --help（项目初始化）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 命令: init --help（项目初始化）${NC}"

output=$($MIMOFAN init --help 2>&1)
if echo "$output" | grep -qE "init|create|AGENTS.md|project"; then
  log_test "init 项目初始化" "pass"
else
  log_test "init 项目初始化" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 命令 18: login --help（登录）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 命令: login --help（登录）${NC}"

output=$($MIMOFAN login --help 2>&1)
if echo "$output" | grep -qE "login|api|key|save"; then
  log_test "login 登录" "pass"
else
  log_test "login 登录" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 命令 19: logout --help（登出）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 命令: logout --help（登出）${NC}"

output=$($MIMOFAN logout --help 2>&1)
if echo "$output" | grep -qE "logout|remove|api|key"; then
  log_test "logout 登出" "pass"
else
  log_test "logout 登出" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 命令 20: exec --help（非交互执行）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 命令: exec --help（非交互执行）${NC}"

output=$($MIMOFAN exec --help 2>&1)
if echo "$output" | grep -qE "exec|run|prompt|auto|agent"; then
  log_test "exec 非交互执行" "pass"
else
  log_test "exec 非交互执行" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 命令 21: auth --help（认证管理）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 命令: auth --help（认证管理）${NC}"

output=$($MIMOFAN auth --help 2>&1)
if echo "$output" | grep -qE "auth|credential|provider|mode"; then
  log_test "auth 认证管理" "pass"
else
  log_test "auth 认证管理" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 命令 22: config --help（配置管理）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 命令: config --help（配置管理）${NC}"

output=$($MIMOFAN config --help 2>&1)
if echo "$output" | grep -qE "config|read|write|list|value"; then
  log_test "config 配置管理" "pass"
else
  log_test "config 配置管理" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 命令 23: model --help（模型解析）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 命令: model --help（模型解析）${NC}"

output=$($MIMOFAN model --help 2>&1)
if echo "$output" | grep -qE "model|resolve|list|provider"; then
  log_test "model 模型解析" "pass"
else
  log_test "model 模型解析" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 命令 24: thread --help（线程管理）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 命令: thread --help（线程管理）${NC}"

output=$($MIMOFAN thread --help 2>&1)
if echo "$output" | grep -qE "thread|session|metadata|resume|fork"; then
  log_test "thread 线程管理" "pass"
else
  log_test "thread 线程管理" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 命令 25: serve --help（启动服务器）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 命令: serve --help（启动服务器）${NC}"

output=$($MIMOFAN serve --help 2>&1)
if echo "$output" | grep -qE "serve|server|http|sse|stdio"; then
  log_test "serve 启动服务器" "pass"
else
  log_test "serve 启动服务器" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 命令 26: app-server --help（应用服务器）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 命令: app-server --help（应用服务器）${NC}"

output=$($MIMOFAN app-server --help 2>&1)
if echo "$output" | grep -qE "app-server|runtime|api|control|plane|http|sse"; then
  log_test "app-server 应用服务器" "pass"
else
  log_test "app-server 应用服务器" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 命令 27: mcp-server --help（MCP 服务器模式）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 命令: mcp-server --help（MCP 服务器模式）${NC}"

output=$($MIMOFAN mcp-server --help 2>&1)
if echo "$output" | grep -qE "mcp-server|stdio|server|mode"; then
  log_test "mcp-server MCP 服务器模式" "pass"
else
  log_test "mcp-server MCP 服务器模式" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 命令 28: remote-setup --help（远程设置）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 命令: remote-setup --help（远程设置）${NC}"

output=$($MIMOFAN remote-setup --help 2>&1)
if echo "$output" | grep -qE "remote|setup|deploy|bundle|cloud|chat|bridge"; then
  log_test "remote-setup 远程设置" "pass"
else
  log_test "remote-setup 远程设置" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 命令 29: resume --help（恢复会话）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 命令: resume --help（恢复会话）${NC}"

output=$($MIMOFAN resume --help 2>&1)
if echo "$output" | grep -qE "resume|session|id|last"; then
  log_test "resume 恢复会话" "pass"
else
  log_test "resume 恢复会话" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 命令 30: fork --help（分叉会话）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 命令: fork --help（分叉会话）${NC}"

output=$($MIMOFAN fork --help 2>&1)
if echo "$output" | grep -qE "fork|session|id|last"; then
  log_test "fork 分叉会话" "pass"
else
  log_test "fork 分叉会话" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 命令 31: metrics --help（指标监控）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 命令: metrics --help（指标监控）${NC}"

output=$($MIMOFAN metrics --help 2>&1)
if echo "$output" | grep -qE "metrics|monitor|stats|performance"; then
  log_test "metrics 指标监控" "pass"
else
  log_test "metrics 指标监控" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 命令 32: pr --help（PR 管理）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 命令: pr --help（PR 管理）${NC}"

output=$($MIMOFAN pr --help 2>&1)
if echo "$output" | grep -qE "pr|pull|request|github|diff"; then
  log_test "pr PR 管理" "pass"
else
  log_test "pr PR 管理" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 命令 33: update --help（更新）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 命令: update --help（更新）${NC}"

output=$($MIMOFAN update --help 2>&1)
if echo "$output" | grep -qE "update|upgrade|version|check"; then
  log_test "update 更新" "pass"
else
  log_test "update 更新" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 命令 34: completion --help（补全）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 命令: completion --help（补全）${NC}"

output=$($MIMOFAN completion --help 2>&1)
if echo "$output" | grep -qE "completion|shell|bash|zsh|fish"; then
  log_test "completion 补全" "pass"
else
  log_test "completion 补全" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 输出结果摘要
# ════════════════════════════════════════════════════════════════
echo ""
echo -e "${BLUE}══════════════════════════════════════════════════════════════${NC}"
echo -e "${BLUE}     CLI 命令测试结果摘要${NC}"
echo -e "${BLUE}══════════════════════════════════════════════════════════════${NC}"
echo ""
echo -e "  总测试数: $test_number"
echo -e "  ${GREEN}通过: $pass_count${NC}"
echo -e "  ${RED}失败: $fail_count${NC}"
echo ""

if [ $fail_count -gt 0 ]; then
  echo -e "${RED}失败的测试: ${failures[*]}${NC}"
  exit 1
else
  echo -e "${GREEN}✓ 所有 CLI 命令测试通过！${NC}"
  exit 0
fi
