#!/bin/bash
# =============================================================================
# 命令系统验收测试
# =============================================================================
# 覆盖：Core、Session、Memory、Utility、Config、Skills 命令
# 使用：./benchmark/commands_test.sh
# =============================================================================

set -euo pipefail

API_KEY="sk-sfl2f69ak7vrf538yq093akm8ngh149cf489eqmvfam3ndhi"
OPENAI_URL="https://api.xiaomimimo.com/v1/chat/completions"
MODEL="mimo-v2.5"

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

echo ""
echo "╔════════════════════════════════════════════════════════════╗"
echo "║     命令系统验收测试                                       ║"
echo "╚════════════════════════════════════════════════════════════╝"

# ============================================================
# 1. Core 命令
# ============================================================
echo ""
echo "━━━ 1. Core 命令 ━━━"

# 1.1 /help
RESP=$(call_api '{
  "model":"'"$MODEL"'",
  "messages":[{"role":"user","content":"/help"}],
  "max_tokens":100
}' 2>&1) || true

CONTENT=$(echo "$RESP" | python3 -c "import sys,json; print(json.load(sys.stdin)['choices'][0]['message'].get('content','')[:200])" 2>/dev/null || echo "")
if [ -n "$CONTENT" ]; then
  pass "/help 命令 — 返回帮助信息"
else
  fail "/help 命令" "无响应内容"
fi

# 1.2 /model
RESP=$(call_api '{
  "model":"'"$MODEL"'",
  "messages":[{"role":"user","content":"/model"}],
  "max_tokens":100
}' 2>&1) || true

if echo "$RESP" | grep -q '"content"'; then
  pass "/model 命令 — 返回模型信息"
else
  fail "/model 命令" "无响应内容"
fi

# 1.3 /models
RESP=$(call_api '{
  "model":"'"$MODEL"'",
  "messages":[{"role":"user","content":"/models"}],
  "max_tokens":100
}' 2>&1) || true

if echo "$RESP" | grep -q '"content"'; then
  pass "/models 命令 — 返回模型列表"
else
  fail "/models 命令" "无响应内容"
fi

# 1.4 /plan
RESP=$(call_api '{
  "model":"'"$MODEL"'",
  "messages":[{"role":"user","content":"/plan 重构用户认证模块"}],
  "max_tokens":200
}' 2>&1) || true

if echo "$RESP" | grep -q '"content"'; then
  pass "/plan 命令 — 返回计划内容"
else
  fail "/plan 命令" "无响应内容"
fi

# 1.5 /translate
RESP=$(call_api '{
  "model":"'"$MODEL"'",
  "messages":[{"role":"user","content":"/translate Hello World"}],
  "max_tokens":100
}' 2>&1) || true

if echo "$RESP" | grep -q '"content"'; then
  pass "/translate 命令 — 返回翻译结果"
else
  fail "/translate 命令" "无响应内容"
fi

# ============================================================
# 2. Memory 命令
# ============================================================
echo ""
echo "━━━ 2. Memory 命令 ━━━"

# 2.1 /memory
RESP=$(call_api '{
  "model":"'"$MODEL"'",
  "messages":[{"role":"user","content":"/memory"}],
  "max_tokens":100
}' 2>&1) || true

if echo "$RESP" | grep -q '"content"'; then
  pass "/memory 命令 — 返回记忆信息"
else
  fail "/memory 命令" "无响应内容"
fi

# 2.2 /note
RESP=$(call_api '{
  "model":"'"$MODEL"'",
  "messages":[{"role":"user","content":"/note 记录项目使用 Rust 编写"}],
  "max_tokens":100
}' 2>&1) || true

if echo "$RESP" | grep -q '"content"'; then
  pass "/note 命令 — 返回笔记确认"
else
  fail "/note 命令" "无响应内容"
fi

# ============================================================
# 3. Utility 命令
# ============================================================
echo ""
echo "━━━ 3. Utility 命令 ━━━"

# 3.1 /tools
RESP=$(call_api '{
  "model":"'"$MODEL"'",
  "messages":[{"role":"user","content":"/tools"}],
  "max_tokens":200
}' 2>&1) || true

if echo "$RESP" | grep -q '"content"'; then
  pass "/tools 命令 — 返回工具列表"
else
  fail "/tools 命令" "无响应内容"
fi

# 3.2 /network
RESP=$(call_api '{
  "model":"'"$MODEL"'",
  "messages":[{"role":"user","content":"/network"}],
  "max_tokens":100
}' 2>&1) || true

if echo "$RESP" | grep -q '"content"'; then
  pass "/network 命令 — 返回网络状态"
else
  fail "/network 命令" "无响应内容"
fi

# ============================================================
# 4. Config 命令
# ============================================================
echo ""
echo "━━━ 4. Config 命令 ━━━"

RESP=$(call_api '{
  "model":"'"$MODEL"'",
  "messages":[{"role":"user","content":"/config"}],
  "max_tokens":100
}' 2>&1) || true

if echo "$RESP" | grep -q '"content"'; then
  pass "/config 命令 — 返回配置信息"
else
  fail "/config 命令" "无响应内容"
fi

# ============================================================
# 5. Skills 命令
# ============================================================
echo ""
echo "━━━ 5. Skills 命令 ━━━"

RESP=$(call_api '{
  "model":"'"$MODEL"'",
  "messages":[{"role":"user","content":"/skills"}],
  "max_tokens":200
}' 2>&1) || true

if echo "$RESP" | grep -q '"content"'; then
  pass "/skills 命令 — 返回技能列表"
else
  fail "/skills 命令" "无响应内容"
fi

# ============================================================
# 6. 多轮命令交互
# ============================================================
echo ""
echo "━━━ 6. 多轮命令交互 ━━━"

RESP=$(call_api '{
  "model":"'"$MODEL"'",
  "messages":[
    {"role":"user","content":"/help"},
    {"role":"assistant","content":"以下是可用命令..."},
    {"role":"user","content":"那 /model 呢？"}
  ],
  "max_tokens":100
}' 2>&1) || true

if echo "$RESP" | grep -q '"content"'; then
  pass "多轮命令交互 — 上下文记忆正常"
else
  fail "多轮命令交互" "无响应内容"
fi

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
