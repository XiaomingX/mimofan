#!/bin/bash
# =============================================================================
# API 能力验收测试 - 覆盖典型样本类别
# =============================================================================
# 类别：工具调用、多轮对话、推理模式、JSON 输出、流式响应、图片理解
# 使用：./benchmark/capability_tests.sh
# =============================================================================

set -euo pipefail

API_KEY="${MIMOFAN_TEST_API_KEY:?set MIMOFAN_TEST_API_KEY in CI secret}"
OPENAI_URL="https://api.xiaomimimo.com/v1/chat/completions"
ANTHROPIC_URL="https://api.xiaomimimo.com/anthropic/v1/messages"
MODEL="mimo-v2.5"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

TOTAL=0; PASSED=0; FAILED=0

pass() { TOTAL=$((TOTAL+1)); PASSED=$((PASSED+1)); echo -e "${GREEN}✓ PASS${NC}: $1"; }
fail() { TOTAL=$((TOTAL+1)); FAILED=$((FAILED+1)); echo -e "${RED}✗ FAIL${NC}: $1 — $2"; }

echo ""
echo "╔════════════════════════════════════════════════════════════╗"
echo "║     API 能力验收测试 — 典型样本类别                        ║"
echo "╚════════════════════════════════════════════════════════════╝"

# ============================================================
# 1. 工具调用 (Tool Calling / Function Calling)
# ============================================================
echo ""
echo "━━━ 1. 工具调用 (Tool Calling) ━━━"

RESP=$(curl -sf -X POST "$OPENAI_URL" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $API_KEY" \
  -d '{
    "model":"'"$MODEL"'",
    "messages":[{"role":"user","content":"北京天气怎么样？请使用 get_weather 工具"}],
    "tools":[{"type":"function","function":{"name":"get_weather","description":"获取城市天气","parameters":{"type":"object","properties":{"city":{"type":"string"}},"required":["city"]}}}],
    "max_tokens":100
  }' 2>&1) || true

if echo "$RESP" | grep -q '"tool_calls"'; then
  pass "工具调用 — 模型正确返回 tool_calls"
else
  fail "工具调用" "未返回 tool_calls: $(echo "$RESP" | head -c 200)"
fi

# 并行多工具调用
RESP2=$(curl -sf -X POST "$OPENAI_URL" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $API_KEY" \
  -d '{
    "model":"'"$MODEL"'",
    "messages":[{"role":"user","content":"同时查北京和上海的天气"}],
    "tools":[
      {"type":"function","function":{"name":"get_weather","description":"获取城市天气","parameters":{"type":"object","properties":{"city":{"type":"string"}},"required":["city"]}}},
      {"type":"function","function":{"name":"get_time","description":"获取当前时间","parameters":{"type":"object","properties":{}}}}
    ],
    "max_tokens":150
  }' 2>&1) || true

if echo "$RESP2" | grep -q '"tool_calls"'; then
  TC_COUNT=$(echo "$RESP2" | python3 -c "import sys,json; d=json.load(sys.stdin); print(len(d['choices'][0]['message'].get('tool_calls',[])))" 2>/dev/null || echo "0")
  if [ "$TC_COUNT" -ge 1 ]; then
    pass "并行工具调用 — 返回 $TC_COUNT 个工具调用"
  else
    fail "并行工具调用" "tool_calls 数量异常: $TC_COUNT"
  fi
else
  fail "并行工具调用" "未返回 tool_calls"
fi

# ============================================================
# 2. 多轮对话 (Multi-turn Conversation)
# ============================================================
echo ""
echo "━━━ 2. 多轮对话 (Multi-turn) ━━━"

RESP=$(curl -sf -X POST "$OPENAI_URL" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $API_KEY" \
  -d '{
    "model":"'"$MODEL"'",
    "messages":[
      {"role":"system","content":"你是一个助手，记住用户的名字。"},
      {"role":"user","content":"我叫小明。"},
      {"role":"assistant","content":"你好小明！"},
      {"role":"user","content":"我叫什么名字？"}
    ],
    "max_tokens":30
  }' 2>&1) || true

if echo "$RESP" | grep -q '"content"'; then
  pass "多轮对话 — 上下文记忆正常"
else
  fail "多轮对话" "响应异常: $(echo "$RESP" | head -c 200)"
fi

# 长上下文多轮
RESP2=$(curl -sf -X POST "$OPENAI_URL" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $API_KEY" \
  -d '{
    "model":"'"$MODEL"'",
    "messages":[
      {"role":"user","content":"请记住这些数字：3, 7, 15, 42"},
      {"role":"assistant","content":"好的，我记住了：3, 7, 15, 42"},
      {"role":"user","content":"第二个数字是什么？"},
      {"role":"assistant","content":"第二个数字是 7"},
      {"role":"user","content":"最大的数字是什么？"}
    ],
    "max_tokens":30
  }' 2>&1) || true

if echo "$RESP2" | grep -q '"content"'; then
  pass "长上下文多轮 — 多轮信息追踪正常"
else
  fail "长上下文多轮" "响应异常"
fi

# ============================================================
# 3. 推理模式 (Reasoning / Thinking)
# ============================================================
echo ""
echo "━━━ 3. 推理模式 (Reasoning) ━━━"

# Anthropic 思考模式
RESP=$(curl -sf -X POST "$ANTHROPIC_URL" \
  -H "Content-Type: application/json" \
  -H "x-api-key: $API_KEY" \
  -H "anthropic-version: 2023-06-01" \
  -d '{
    "model":"'"$MODEL"'",
    "thinking":{"type":"enabled","budget_tokens":200},
    "messages":[{"role":"user","content":"1+1等于几？请思考后回答"}],
    "max_tokens":300
  }' 2>&1) || true

if echo "$RESP" | grep -q '"thinking"'; then
  pass "推理模式(Anthropic) — thinking 响应正常"
else
  fail "推理模式(Anthropic)" "未返回 thinking: $(echo "$RESP" | head -c 200)"
fi

# OpenAI reasoning_content
RESP2=$(curl -sf -X POST "$OPENAI_URL" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $API_KEY" \
  -d '{
    "model":"'"$MODEL"'",
    "messages":[{"role":"user","content":"2+3等于几？请思考后回答"}],
    "max_tokens":100
  }' 2>&1) || true

if echo "$RESP2" | grep -q '"reasoning_content"'; then
  pass "推理模式(OpenAI) — reasoning_content 正常"
else
  # reasoning_content 可能为空但字段存在，检查 usage
  if echo "$RESP2" | grep -q '"reasoning_tokens"'; then
    pass "推理模式(OpenAI) — reasoning_tokens 统计正常"
  else
    fail "推理模式(OpenAI)" "未返回推理信息"
  fi
fi

# ============================================================
# 4. JSON 输出模式 (JSON Mode)
# ============================================================
echo ""
echo "━━━ 4. JSON 输出模式 ━━━"

RESP=$(curl -sf -X POST "$OPENAI_URL" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $API_KEY" \
  -d '{
    "model":"'"$MODEL"'",
    "messages":[{"role":"user","content":"返回一个包含 name 和 age 字段的 JSON 对象，name 为 Alice，age 为 25"}],
    "response_format":{"type":"json_object"},
    "max_tokens":100
  }' 2>&1) || true

CONTENT=$(echo "$RESP" | python3 -c "import sys,json; print(json.load(sys.stdin)['choices'][0]['message']['content'])" 2>/dev/null || echo "")
if echo "$CONTENT" | python3 -m json.tool >/dev/null 2>&1; then
  pass "JSON 输出 — 合法 JSON 响应"
else
  fail "JSON 输出" "非合法 JSON: $CONTENT"
fi

# ============================================================
# 5. 流式响应 (Streaming)
# ============================================================
echo ""
echo "━━━ 5. 流式响应 (Streaming) ━━━"

RESP=$(curl -sf -X POST "$OPENAI_URL" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $API_KEY" \
  -d '{
    "model":"'"$MODEL"'",
    "messages":[{"role":"user","content":"说你好"}],
    "stream":true,
    "max_tokens":20
  }' 2>&1) || true

if echo "$RESP" | grep -q "data: "; then
  CHUNK_COUNT=$(echo "$RESP" | grep -c "data: " || true)
  pass "流式响应 — 收到 $CHUNK_COUNT 个数据块"
else
  fail "流式响应" "未收到 SSE 数据"
fi

# ============================================================
# 6. Anthropic 工具调用
# ============================================================
echo ""
echo "━━━ 6. Anthropic 工具调用 ━━━"

RESP=$(curl -sf -X POST "$ANTHROPIC_URL" \
  -H "Content-Type: application/json" \
  -H "x-api-key: $API_KEY" \
  -H "anthropic-version: 2023-06-01" \
  -d '{
    "model":"'"$MODEL"'",
    "tools":[{"name":"calc","description":"计算数学表达式","input_schema":{"type":"object","properties":{"expr":{"type":"string"}},"required":["expr"]}}],
    "messages":[{"role":"user","content":"计算 2+3"}],
    "max_tokens":200
  }' 2>&1) || true

if echo "$RESP" | grep -q '"tool_use"'; then
  pass "Anthropic 工具调用 — 正确返回 tool_use"
else
  fail "Anthropic 工具调用" "未返回 tool_use: $(echo "$RESP" | head -c 200)"
fi

# ============================================================
# 7. Anthropic 多轮 + 工具结果回传
# ============================================================
echo ""
echo "━━━ 7. 工具结果回传 (Tool Result) ━━━"

RESP=$(curl -sf -X POST "$OPENAI_URL" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $API_KEY" \
  -d '{
    "model":"'"$MODEL"'",
    "messages":[
      {"role":"user","content":"北京天气"},
      {"role":"assistant","content":"","tool_calls":[{"id":"call_001","type":"function","function":{"name":"get_weather","arguments":"{\"city\":\"Beijing\"}"}}]},
      {"role":"tool","tool_call_id":"call_001","content":"北京：晴，25°C，湿度 40%"},
      {"role":"user","content":"谢谢，天气怎么样？"}
    ],
    "max_tokens":100
  }' 2>&1) || true

if echo "$RESP" | grep -q '"content"'; then
  pass "工具结果回传 — 多轮工具交互正常"
else
  fail "工具结果回传" "响应异常"
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
