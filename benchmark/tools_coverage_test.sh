#!/bin/bash
# =============================================================================
# 工具覆盖补全测试 — MECE 原则
# =============================================================================
# 补全 benchmark/tools_test.sh 未覆盖的模型可见工具
# 遵循奥卡姆剃刀：仅测试模型可调用的工具，跳过内部辅助函数
# =============================================================================

set -euo pipefail

API_KEY="sk-sfl2f69ak7vrf538yq093akm8ngh149cf489eqmvfam3ndhi"
OPENAI_URL="https://api.xiaomimimo.com/v1/chat/completions"
ANTHROPIC_URL="https://api.xiaomimimo.com/anthropic/v1/messages"
MODEL="mimo-v2.5"

RED='\033[0;31m'; GREEN='\033[0;32m'; NC='\033[0m'
TOTAL=0; PASSED=0; FAILED=0

pass() { TOTAL=$((TOTAL+1)); PASSED=$((PASSED+1)); echo -e "${GREEN}✓ PASS${NC}: $1"; }
fail() { TOTAL=$((TOTAL+1)); FAILED=$((FAILED+1)); echo -e "${RED}✗ FAIL${NC}: $1 — $2"; }

call_openai() {
  curl -sf -X POST "$OPENAI_URL" \
    -H "Content-Type: application/json" \
    -H "Authorization: Bearer $API_KEY" \
    -d "$1" 2>&1
}

call_anthropic() {
  curl -sf -X POST "$ANTHROPIC_URL" \
    -H "Content-Type: application/json" \
    -H "x-api-key: $API_KEY" \
    -H "anthropic-version: 2023-06-01" \
    -d "$1" 2>&1
}

has_tool_calls() { echo "$1" | grep -q '"tool_calls"'; }

echo ""
echo "╔════════════════════════════════════════════════════════════╗"
echo "║     工具覆盖补全测试 — MECE 原则                           ║"
echo "╚════════════════════════════════════════════════════════════╝"

# ============================================================
# 1. 文件操作（补全 apply_patch）
# ============================================================
echo ""
echo "━━━ 1. 文件操作（补全） ━━━"

RESP=$(call_openai '{
  "model":"'"$MODEL"'",
  "messages":[{"role":"user","content":"在 /tmp/test.txt 文件开头添加一行 \"version 2\""}],
  "tools":[{"type":"function","function":{"name":"apply_patch","description":"应用补丁修改文件","parameters":{"type":"object","properties":{"path":{"type":"string"},"patch":{"type":"string"}},"required":["path","patch"]}}}],
  "max_tokens":100
}' 2>&1) || true

if has_tool_calls "$RESP"; then
  pass "apply_patch 工具 — 正确触发补丁应用"
else
  fail "apply_patch 工具" "未触发 tool_calls"
fi

# ============================================================
# 2. Git 历史（补全 git_history）
# ============================================================
echo ""
echo "━━━ 2. Git 历史（补全） ━━━"

# 2.1 git_log
RESP=$(call_openai '{
  "model":"'"$MODEL"'",
  "messages":[{"role":"user","content":"查看最近 5 条 git 提交记录"}],
  "tools":[{"type":"function","function":{"name":"git_log","description":"查看 git 提交历史","parameters":{"type":"object","properties":{"count":{"type":"integer"},"path":{"type":"string"}},"required":[]}}}],
  "max_tokens":100
}' 2>&1) || true

if has_tool_calls "$RESP"; then
  pass "git_log 工具 — 正确触发提交历史查询"
else
  fail "git_log 工具" "未触发 tool_calls"
fi

# 2.2 git_blame
RESP=$(call_openai '{
  "model":"'"$MODEL"'",
  "messages":[{"role":"user","content":"查看 README.md 的 git blame"}],
  "tools":[{"type":"function","function":{"name":"git_blame","description":"查看文件 git blame","parameters":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}}}],
  "max_tokens":100
}' 2>&1) || true

if has_tool_calls "$RESP"; then
  pass "git_blame 工具 — 正确触发 blame 查询"
else
  fail "git_blame 工具" "未触发 tool_calls"
fi

# ============================================================
# 3. Web 执行（补全 web_run）
# ============================================================
echo ""
echo "━━━ 3. Web 执行（补全） ━━━"

RESP=$(call_openai '{
  "model":"'"$MODEL"'",
  "messages":[{"role":"user","content":"执行 https://httpbin.org/get 并返回结果"}],
  "tools":[{"type":"function","function":{"name":"web.run","description":"执行网页并返回内容","parameters":{"type":"object","properties":{"url":{"type":"string"}},"required":["url"]}}}],
  "max_tokens":100
}' 2>&1) || true

if has_tool_calls "$RESP"; then
  pass "web.run 工具 — 正确触发网页执行"
else
  fail "web.run 工具" "未触发 tool_calls"
fi

# ============================================================
# 4. Goal 工具（补全 goal）
# ============================================================
echo ""
echo "━━━ 4. Goal 工具（补全） ━━━"

# 4.1 create_goal
RESP=$(call_openai '{
  "model":"'"$MODEL"'",
  "messages":[{"role":"user","content":"创建一个目标：重构用户认证模块"}],
  "tools":[{"type":"function","function":{"name":"create_goal","description":"创建目标","parameters":{"type":"object","properties":{"title":{"type":"string"},"description":{"type":"string"}},"required":["title"]}}}],
  "max_tokens":100
}' 2>&1) || true

if has_tool_calls "$RESP"; then
  pass "create_goal 工具 — 正确触发目标创建"
else
  fail "create_goal 工具" "未触发 tool_calls"
fi

# 4.2 get_goal
RESP=$(call_openai '{
  "model":"'"$MODEL"'",
  "messages":[{"role":"user","content":"查看当前目标"}],
  "tools":[{"type":"function","function":{"name":"get_goal","description":"获取当前目标","parameters":{"type":"object","properties":{}}}}],
  "max_tokens":100
}' 2>&1) || true

if has_tool_calls "$RESP"; then
  pass "get_goal 工具 — 正确触发目标查询"
else
  fail "get_goal 工具" "未触发 tool_calls"
fi

# ============================================================
# 5. Task 工具（补全 tasks）
# ============================================================
echo ""
echo "━━━ 5. Task 工具（补全） ━━━"

# 5.1 task_create
RESP=$(call_openai '{
  "model":"'"$MODEL"'",
  "messages":[{"role":"user","content":"创建一个任务：编写单元测试"}],
  "tools":[{"type":"function","function":{"name":"task_create","description":"创建任务","parameters":{"type":"object","properties":{"subject":{"type":"string"},"description":{"type":"string"}},"required":["subject"]}}}],
  "max_tokens":100
}' 2>&1) || true

if has_tool_calls "$RESP"; then
  pass "task_create 工具 — 正确触发任务创建"
else
  fail "task_create 工具" "未触发 tool_calls"
fi

# 5.2 task_list
RESP=$(call_openai '{
  "model":"'"$MODEL"'",
  "messages":[{"role":"user","content":"列出所有任务"}],
  "tools":[{"type":"function","function":{"name":"task_list","description":"列出任务","parameters":{"type":"object","properties":{}}}}],
  "max_tokens":100
}' 2>&1) || true

if has_tool_calls "$RESP"; then
  pass "task_list 工具 — 正确触发任务列表"
else
  fail "task_list 工具" "未触发 tool_calls"
fi

# ============================================================
# 6. 代码质量（补全 verifier、test_runner）
# ============================================================
echo ""
echo "━━━ 6. 代码质量（补全） ━━━"

# 6.1 run_tests
RESP=$(call_openai '{
  "model":"'"$MODEL"'",
  "messages":[{"role":"user","content":"运行项目的单元测试"}],
  "tools":[{"type":"function","function":{"name":"run_tests","description":"运行测试","parameters":{"type":"object","properties":{"filter":{"type":"string"}},"required":[]}}}],
  "max_tokens":100
}' 2>&1) || true

if has_tool_calls "$RESP"; then
  pass "run_tests 工具 — 正确触发测试运行"
else
  fail "run_tests 工具" "未触发 tool_calls"
fi

# 6.2 run_verifiers
RESP=$(call_openai '{
  "model":"'"$MODEL"'",
  "messages":[{"role":"user","content":"运行代码验证器"}],
  "tools":[{"type":"function","function":{"name":"run_verifiers","description":"运行验证器","parameters":{"type":"object","properties":{"scope":{"type":"string"}},"required":[]}}}],
  "max_tokens":100
}' 2>&1) || true

if has_tool_calls "$RESP"; then
  pass "run_verifiers 工具 — 正确触发验证器"
else
  fail "run_verifiers 工具" "未触发 tool_calls"
fi

# ============================================================
# 7. 其他工具（补全 image_ocr、finance、notify）
# ============================================================
echo ""
echo "━━━ 7. 其他工具（补全） ━━━"

# 7.1 image_ocr
RESP=$(call_openai '{
  "model":"'"$MODEL"'",
  "messages":[{"role":"user","content":"识别 /tmp/screenshot.png 中的文字"}],
  "tools":[{"type":"function","function":{"name":"image_ocr","description":"图片文字识别","parameters":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}}}],
  "max_tokens":100
}' 2>&1) || true

if has_tool_calls "$RESP"; then
  pass "image_ocr 工具 — 正确触发 OCR"
else
  fail "image_ocr 工具" "未触发 tool_calls"
fi

# 7.2 finance
RESP=$(call_openai '{
  "model":"'"$MODEL"'",
  "messages":[{"role":"user","content":"查询苹果公司股价"}],
  "tools":[{"type":"function","function":{"name":"finance","description":"金融数据查询","parameters":{"type":"object","properties":{"symbol":{"type":"string"},"data_type":{"type":"string"}},"required":["symbol"]}}}],
  "max_tokens":100
}' 2>&1) || true

if has_tool_calls "$RESP"; then
  pass "finance 工具 — 正确触发金融查询"
else
  fail "finance 工具" "未触发 tool_calls"
fi

# 7.3 notify
RESP=$(call_openai '{
  "model":"'"$MODEL"'",
  "messages":[{"role":"user","content":"发送通知：任务已完成"}],
  "tools":[{"type":"function","function":{"name":"notify","description":"发送通知","parameters":{"type":"object","properties":{"message":{"type":"string"},"level":{"type":"string","enum":["info","warning","error"]}},"required":["message"]}}}],
  "max_tokens":100
}' 2>&1) || true

if has_tool_calls "$RESP"; then
  pass "notify 工具 — 正确触发通知"
else
  fail "notify 工具" "未触发 tool_calls"
fi

# ============================================================
# 8. 多工具组合（补全）
# ============================================================
echo ""
echo "━━━ 8. 多工具组合（补全） ━━━"

# 8.1 目标 + 任务组合
RESP=$(call_openai '{
  "model":"'"$MODEL"'",
  "messages":[{"role":"user","content":"创建一个目标 \"重构认证模块\"，然后为它创建 3 个子任务"}],
  "tools":[
    {"type":"function","function":{"name":"create_goal","description":"创建目标","parameters":{"type":"object","properties":{"title":{"type":"string"}},"required":["title"]}}},
    {"type":"function","function":{"name":"task_create","description":"创建任务","parameters":{"type":"object","properties":{"subject":{"type":"string"}},"required":["subject"]}}}
  ],
  "max_tokens":200
}' 2>&1) || true

if has_tool_calls "$RESP"; then
  TC_COUNT=$(echo "$RESP" | python3 -c "import sys,json; d=json.load(sys.stdin); print(len(d['choices'][0]['message'].get('tool_calls',[])))" 2>/dev/null || echo "0")
  pass "目标+任务组合 — 触发 $TC_COUNT 个工具调用"
else
  fail "目标+任务组合" "未触发 tool_calls"
fi

# 8.2 Git + 文件组合
RESP=$(call_openai '{
  "model":"'"$MODEL"'",
  "messages":[{"role":"user","content":"查看 README.md 的 git blame，然后用 apply_patch 修改其中的版本号"}],
  "tools":[
    {"type":"function","function":{"name":"git_blame","description":"查看 git blame","parameters":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}}},
    {"type":"function","function":{"name":"apply_patch","description":"应用补丁","parameters":{"type":"object","properties":{"path":{"type":"string"},"patch":{"type":"string"}},"required":["path","patch"]}}}
  ],
  "max_tokens":150
}' 2>&1) || true

if has_tool_calls "$RESP"; then
  pass "Git+文件组合 — 正确触发多工具"
else
  fail "Git+文件组合" "未触发 tool_calls"
fi

# ============================================================
# 9. Anthropic 工具组合（补全）
# ============================================================
echo ""
echo "━━━ 9. Anthropic 工具组合 ━━━"

RESP=$(curl -sf -X POST "$ANTHROPIC_URL" \
  -H "Content-Type: application/json" \
  -H "x-api-key: $API_KEY" \
  -H "anthropic-version: 2023-06-01" \
  -d '{
    "model": "'"$MODEL"'",
    "tools": [
      {
        "name": "task_create",
        "description": "创建任务",
        "input_schema": {
          "type": "object",
          "properties": {"subject": {"type": "string"}},
          "required": ["subject"]
        }
      },
      {
        "name": "run_tests",
        "description": "运行测试",
        "input_schema": {
          "type": "object",
          "properties": {"filter": {"type": "string"}},
          "required": []
        }
      }
    ],
    "messages": [{"role": "user", "content": "创建一个任务并运行测试"}],
    "max_tokens": 200
  }' 2>&1) || true

if echo "$RESP" | grep -q '"tool_use"'; then
  pass "Anthropic 工具组合 — 正确触发 tool_use"
else
  fail "Anthropic 工具组合" "未触发 tool_use"
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
