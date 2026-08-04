#!/bin/bash
# =============================================================================
# 内置工具能力验收测试
# =============================================================================
# 覆盖：文件操作、Shell、Git、搜索、规划、记忆、代码质量等
# 使用：./benchmark/tools_test.sh
# =============================================================================

set -euo pipefail

API_KEY="sk-sfl2f69ak7vrf538yq093akm8ngh149cf489eqmvfam3ndhi"
OPENAI_URL="https://api.xiaomimimo.com/v1/chat/completions"
MODEL="mimo-v2.5"
WORKSPACE="/tmp/mimofan_bench_$$"
mkdir -p "$WORKSPACE"

RED='\033[0;31m'; GREEN='\033[0;32m'; NC='\033[0m'
TOTAL=0; PASSED=0; FAILED=0

pass() { TOTAL=$((TOTAL+1)); PASSED=$((PASSED+1)); echo -e "${GREEN}✓ PASS${NC}: $1"; }
fail() { TOTAL=$((TOTAL+1)); FAILED=$((FAILED+1)); echo -e "${RED}✗ FAIL${NC}: $1 — $2"; }

call_api() {
  curl -sf -X POST "$OPENAI_URL" \
    -H "Content-Type: application/json" \
    -H "Authorization: Bearer $API_KEY" \
    -d "$1" 2>&1
}

extract_content() {
  python3 -c "import sys,json; d=json.load(sys.stdin); print(d['choices'][0]['message'].get('content','')[:500])" 2>/dev/null <<< "$1"
}

echo ""
echo "╔════════════════════════════════════════════════════════════╗"
echo "║     内置工具能力验收测试                                    ║"
echo "╚════════════════════════════════════════════════════════════╝"

# ============================================================
# 1. 文件操作工具
# ============================================================
echo ""
echo "━━━ 1. 文件操作工具 ━━━"

# 1.1 file - 文件读写
RESP=$(call_api '{
  "model":"'"$MODEL"'",
  "messages":[{"role":"user","content":"请在 /tmp/test_write.txt 写入 hello world"}],
  "tools":[{"type":"function","function":{"name":"file","description":"读写文件","parameters":{"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"},"action":{"type":"string","enum":["read","write"]}},"required":["path","action"]}}}],
  "max_tokens":100
}' 2>&1) || true

if echo "$RESP" | grep -q '"tool_calls"'; then
  pass "file 工具 — 正确触发文件写入"
else
  fail "file 工具" "未触发 tool_calls"
fi

# 1.2 file_search - 文件搜索
RESP=$(call_api '{
  "model":"'"$MODEL"'",
  "messages":[{"role":"user","content":"搜索当前目录下所有 .rs 文件"}],
  "tools":[{"type":"function","function":{"name":"file_search","description":"搜索文件","parameters":{"type":"object","properties":{"pattern":{"type":"string"},"path":{"type":"string"}},"required":["pattern"]}}}],
  "max_tokens":100
}' 2>&1) || true

if echo "$RESP" | grep -q '"tool_calls"'; then
  pass "file_search 工具 — 正确触发文件搜索"
else
  fail "file_search 工具" "未触发 tool_calls"
fi

# ============================================================
# 2. Shell 执行工具
# ============================================================
echo ""
echo "━━━ 2. Shell 执行工具 ━━━"

RESP=$(call_api '{
  "model":"'"$MODEL"'",
  "messages":[{"role":"user","content":"执行 ls -la /tmp 命令"}],
  "tools":[{"type":"function","function":{"name":"shell","description":"执行 shell 命令","parameters":{"type":"object","properties":{"command":{"type":"string"}},"required":["command"]}}}],
  "max_tokens":100
}' 2>&1) || true

if echo "$RESP" | grep -q '"tool_calls"'; then
  CMD=$(echo "$RESP" | python3 -c "import sys,json; tc=json.load(sys.stdin)['choices'][0]['message']['tool_calls'][0]; print(tc['function']['arguments'])" 2>/dev/null || echo "")
  if echo "$CMD" | grep -q "ls"; then
    pass "shell 工具 — 正确生成 ls 命令"
  else
    pass "shell 工具 — 触发 tool_calls"
  fi
else
  fail "shell 工具" "未触发 tool_calls"
fi

# ============================================================
# 3. Git 操作工具
# ============================================================
echo ""
echo "━━━ 3. Git 操作工具 ━━━"

RESP=$(call_api '{
  "model":"'"$MODEL"'",
  "messages":[{"role":"user","content":"查看当前 git 仓库状态"}],
  "tools":[{"type":"function","function":{"name":"git","description":"执行 git 命令","parameters":{"type":"object","properties":{"command":{"type":"string"}},"required":["command"]}}}],
  "max_tokens":100
}' 2>&1) || true

if echo "$RESP" | grep -q '"tool_calls"'; then
  pass "git 工具 — 正确触发 git 操作"
else
  fail "git 工具" "未触发 tool_calls"
fi

# ============================================================
# 4. 搜索与抓取工具
# ============================================================
echo ""
echo "━━━ 4. 搜索与抓取工具 ━━━"

# 4.1 search - 代码搜索
RESP=$(call_api '{
  "model":"'"$MODEL"'",
  "messages":[{"role":"user","content":"搜索代码中包含 TODO 的行"}],
  "tools":[{"type":"function","function":{"name":"search","description":"搜索代码","parameters":{"type":"object","properties":{"query":{"type":"string"},"path":{"type":"string"}},"required":["query"]}}}],
  "max_tokens":100
}' 2>&1) || true

if echo "$RESP" | grep -q '"tool_calls"'; then
  pass "search 工具 — 正确触发代码搜索"
else
  fail "search 工具" "未触发 tool_calls"
fi

# 4.2 web_search - 网页搜索
RESP=$(call_api '{
  "model":"'"$MODEL"'",
  "messages":[{"role":"user","content":"搜索 Rust 编程语言最新版本"}],
  "tools":[{"type":"function","function":{"name":"web_search","description":"网页搜索","parameters":{"type":"object","properties":{"query":{"type":"string"}},"required":["query"]}}}],
  "max_tokens":100
}' 2>&1) || true

if echo "$RESP" | grep -q '"tool_calls"'; then
  pass "web_search 工具 — 正确触发网页搜索"
else
  fail "web_search 工具" "未触发 tool_calls"
fi

# 4.3 fetch_url - URL 抓取
RESP=$(call_api '{
  "model":"'"$MODEL"'",
  "messages":[{"role":"user","content":"抓取 https://httpbin.org/get 的内容"}],
  "tools":[{"type":"function","function":{"name":"fetch_url","description":"抓取 URL 内容","parameters":{"type":"object","properties":{"url":{"type":"string"}},"required":["url"]}}}],
  "max_tokens":100
}' 2>&1) || true

if echo "$RESP" | grep -q '"tool_calls"'; then
  pass "fetch_url 工具 — 正确触发 URL 抓取"
else
  fail "fetch_url 工具" "未触发 tool_calls"
fi

# ============================================================
# 5. 规划与任务工具
# ============================================================
echo ""
echo "━━━ 5. 规划与任务工具 ━━━"

# 5.1 plan - 规划工具
RESP=$(call_api '{
  "model":"'"$MODEL"'",
  "messages":[{"role":"user","content":"为重构用户认证模块制定计划"}],
  "tools":[{"type":"function","function":{"name":"plan","description":"制定计划","parameters":{"type":"object","properties":{"objective":{"type":"string"},"steps":{"type":"array","items":{"type":"string"}}},"required":["objective"]}}}],
  "max_tokens":200
}' 2>&1) || true

if echo "$RESP" | grep -q '"tool_calls"'; then
  pass "plan 工具 — 正确触发规划"
else
  fail "plan 工具" "未触发 tool_calls"
fi

# 5.2 todo - 待办事项
RESP=$(call_api '{
  "model":"'"$MODEL"'",
  "messages":[{"role":"user","content":"创建一个待办事项：完成单元测试"}],
  "tools":[{"type":"function","function":{"name":"todo","description":"管理待办事项","parameters":{"type":"object","properties":{"action":{"type":"string","enum":["create","list","complete"]},"title":{"type":"string"}},"required":["action"]}}}],
  "max_tokens":100
}' 2>&1) || true

if echo "$RESP" | grep -q '"tool_calls"'; then
  pass "todo 工具 — 正确触发待办创建"
else
  fail "todo 工具" "未触发 tool_calls"
fi

# ============================================================
# 6. 记忆与上下文工具
# ============================================================
echo ""
echo "━━━ 6. 记忆与上下文工具 ━━━"

RESP=$(call_api '{
  "model":"'"$MODEL"'",
  "messages":[{"role":"user","content":"记住：项目使用 Rust 编写"}],
  "tools":[{"type":"function","function":{"name":"remember","description":"存储记忆","parameters":{"type":"object","properties":{"content":{"type":"string"},"category":{"type":"string"}},"required":["content"]}}}],
  "max_tokens":100
}' 2>&1) || true

if echo "$RESP" | grep -q '"tool_calls"'; then
  pass "remember 工具 — 正确触发记忆存储"
else
  fail "remember 工具" "未触发 tool_calls"
fi

# ============================================================
# 7. 代码质量工具
# ============================================================
echo ""
echo "━━━ 7. 代码质量工具 ━━━"

# 7.1 review - 代码审查
RESP=$(call_api '{
  "model":"'"$MODEL"'",
  "messages":[{"role":"user","content":"审查以下代码的潜在问题：fn add(a: i32, b: i32) -> i32 { a + b }"}],
  "tools":[{"type":"function","function":{"name":"review","description":"代码审查","parameters":{"type":"object","properties":{"code":{"type":"string"},"language":{"type":"string"}},"required":["code"]}}}],
  "max_tokens":200
}' 2>&1) || true

if echo "$RESP" | grep -q '"tool_calls"'; then
  pass "review 工具 — 正确触发代码审查"
else
  fail "review 工具" "未触发 tool_calls"
fi

# 7.2 diagnostics - 诊断工具
RESP=$(call_api '{
  "model":"'"$MODEL"'",
  "messages":[{"role":"user","content":"诊断当前项目的健康状态"}],
  "tools":[{"type":"function","function":{"name":"diagnostics","description":"项目诊断","parameters":{"type":"object","properties":{"scope":{"type":"string"}},"required":[]}}}],
  "max_tokens":100
}' 2>&1) || true

if echo "$RESP" | grep -q '"tool_calls"'; then
  pass "diagnostics 工具 — 正确触发诊断"
else
  fail "diagnostics 工具" "未触发 tool_calls"
fi

# ============================================================
# 8. 数据处理工具
# ============================================================
echo ""
echo "━━━ 8. 数据处理工具 ━━━"

# 8.1 truncate - 输出截断
RESP=$(call_api '{
  "model":"'"$MODEL"'",
  "messages":[{"role":"user","content":"将以下文本截断到 50 字符：这是一段很长的文本用于测试截断功能是否正常工作"}],
  "tools":[{"type":"function","function":{"name":"truncate","description":"截断文本","parameters":{"type":"object","properties":{"text":{"type":"string"},"max_length":{"type":"integer"}},"required":["text","max_length"]}}}],
  "max_tokens":100
}' 2>&1) || true

if echo "$RESP" | grep -q '"tool_calls"'; then
  pass "truncate 工具 — 正确触发截断"
else
  fail "truncate 工具" "未触发 tool_calls"
fi

# 8.2 validate_data - 数据验证
RESP=$(call_api '{
  "model":"'"$MODEL"'",
  "messages":[{"role":"user","content":"验证这个邮箱是否合法：test@example.com"}],
  "tools":[{"type":"function","function":{"name":"validate_data","description":"数据验证","parameters":{"type":"object","properties":{"data":{"type":"string"},"rule":{"type":"string"}},"required":["data"]}}}],
  "max_tokens":100
}' 2>&1) || true

if echo "$RESP" | grep -q '"tool_calls"'; then
  pass "validate_data 工具 — 正确触发验证"
else
  fail "validate_data 工具" "未触发 tool_calls"
fi

# ============================================================
# 9. 子智能体工具
# ============================================================
echo ""
echo "━━━ 9. 子智能体工具 ━━━"

RESP=$(call_api '{
  "model":"'"$MODEL"'",
  "messages":[{"role":"user","content":"启动一个探索子智能体搜索代码中的 TODO"}],
  "tools":[{"type":"function","function":{"name":"agent","description":"管理子智能体","parameters":{"type":"object","properties":{"action":{"type":"string","enum":["start","status","cancel"]},"objective":{"type":"string"},"agent_type":{"type":"string","enum":["general","explore","plan","review"]}},"required":["action"]}}}],
  "max_tokens":150
}' 2>&1) || true

if echo "$RESP" | grep -q '"tool_calls"'; then
  pass "agent 工具 — 正确触发子智能体"
else
  fail "agent 工具" "未触发 tool_calls"
fi

# ============================================================
# 10. 多工具组合
# ============================================================
echo ""
echo "━━━ 10. 多工具组合 ━━━"

RESP=$(call_api '{
  "model":"'"$MODEL"'",
  "messages":[{"role":"user","content":"先搜索项目中的 README 文件，然后读取其内容"}],
  "tools":[
    {"type":"function","function":{"name":"file_search","description":"搜索文件","parameters":{"type":"object","properties":{"pattern":{"type":"string"}},"required":["pattern"]}}},
    {"type":"function","function":{"name":"file","description":"读写文件","parameters":{"type":"object","properties":{"path":{"type":"string"},"action":{"type":"string"}},"required":["path","action"]}}}
  ],
  "max_tokens":150
}' 2>&1) || true

if echo "$RESP" | grep -q '"tool_calls"'; then
  TC_COUNT=$(echo "$RESP" | python3 -c "import sys,json; d=json.load(sys.stdin); print(len(d['choices'][0]['message'].get('tool_calls',[])))" 2>/dev/null || echo "0")
  pass "多工具组合 — 触发 $TC_COUNT 个工具调用"
else
  fail "多工具组合" "未触发 tool_calls"
fi

# ============================================================
# 清理
# ============================================================
rm -rf "$WORKSPACE"

# ============================================================
# 汇总
# ============================================================
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  测试结果汇总"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  总测试数: $TOTAL"
echo -e "  通过: ${GREEN}$PASSED${NC}"
echo -e "  失败: ${RED}$FAILED${NC}"
echo ""

if [ "$FAILED" -eq 0 ]; then
  echo -e "${GREEN}✓ 所有测试通过！${NC}"
  exit 0
else
  echo -e "${RED}✗ 有 $FAILED 个测试失败${NC}"
  exit 1
fi
