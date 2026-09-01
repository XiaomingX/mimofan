#!/bin/bash
# API 端点验收测试脚本
# 测试两种 API 端点是否都能正常工作：Anthropic Messages API 和 OpenAI Chat Completions API

set -e

# 加载配置
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/config.env"

echo "=== API 端点验收测试 ==="
echo ""

# 测试 1: Anthropic Messages API
echo "测试 1: Anthropic Messages API"
echo "  端点: ${ANTHROPIC_BASE_URL}/v1/messages"
echo ""

RESPONSE=$(curl -s -w "\n%{http_code}" \
  "${ANTHROPIC_BASE_URL}/v1/messages" \
  -H "content-type: application/json" \
  -H "x-api-key: ${API_KEY}" \
  -H "anthropic-version: 2023-06-01" \
  -d "{
    \"model\": \"${MODEL}\",
    \"max_tokens\": 100,
    \"messages\": [{\"role\": \"user\", \"content\": \"Say hello in 5 words\"}]
  }")

HTTP_CODE=$(echo "$RESPONSE" | tail -n1)
BODY=$(echo "$RESPONSE" | sed '$d')

if [ "$HTTP_CODE" -eq 200 ]; then
  echo "  ✅ 成功 (HTTP $HTTP_CODE)"
  echo "  响应: $(echo "$BODY" | head -c 200)..."
else
  echo "  ❌ 失败 (HTTP $HTTP_CODE)"
  echo "  错误: $BODY"
fi
echo ""

# 测试 2: OpenAI Chat Completions API
echo "测试 2: OpenAI Chat Completions API"
echo "  端点: ${OPENAI_BASE_URL}/chat/completions"
echo ""

RESPONSE=$(curl -s -w "\n%{http_code}" \
  "${OPENAI_BASE_URL}/chat/completions" \
  -H "content-type: application/json" \
  -H "Authorization: Bearer ${API_KEY}" \
  -d "{
    \"model\": \"${MODEL}\",
    \"max_tokens\": 100,
    \"messages\": [{\"role\": \"user\", \"content\": \"Say hello in 5 words\"}]
  }")

HTTP_CODE=$(echo "$RESPONSE" | tail -n1)
BODY=$(echo "$RESPONSE" | sed '$d')

if [ "$HTTP_CODE" -eq 200 ]; then
  echo "  ✅ 成功 (HTTP $HTTP_CODE)"
  echo "  响应: $(echo "$BODY" | head -c 200)..."
else
  echo "  ❌ 失败 (HTTP $HTTP_CODE)"
  echo "  错误: $BODY"
fi
echo ""

echo "=== 测试完成 ==="
