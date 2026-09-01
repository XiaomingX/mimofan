#!/usr/bin/env bash
# ============================================================================
# MiMo 双端点验收测试
# 测试 Xiaomi MiMo 的两个 API 端点：
# 1. https://api.xiaomimimo.com/v1 - OpenAI Chat Completions API 兼容
# 2. https://api.xiaomimimo.com/anthropic - Anthropic Messages API 兼容
# ============================================================================

set -euo pipefail

# 颜色输出
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# 检查 API Key
if [[ -z "${XIAOMI_MIMO_API_KEY:-}" ]]; then
    echo -e "${RED}错误: 未设置 XIAOMI_MIMO_API_KEY 环境变量${NC}"
    echo "请先设置: export XIAOMI_MIMO_API_KEY=your_api_key"
    exit 1
fi

# 测试函数
test_openai_endpoint() {
    echo -e "${YELLOW}测试 OpenAI Chat Completions 端点...${NC}"
    echo "端点: https://api.xiaomimimo.com/v1/chat/completions"

    local response
    response=$(curl -s -w "\n%{http_code}" \
        -X POST "https://api.xiaomimimo.com/v1/chat/completions" \
        -H "Authorization: Bearer ${XIAOMI_MIMO_API_KEY}" \
        -H "Content-Type: application/json" \
        -d '{
            "model": "mimo-v2.5-pro",
            "messages": [{"role": "user", "content": "Say hello in one sentence."}],
            "max_tokens": 100
        }' 2>&1)

    local http_code
    http_code=$(echo "$response" | tail -n1)
    local body
    body=$(echo "$response" | head -n-1)

    if [[ "$http_code" -eq 200 ]]; then
        echo -e "${GREEN}✓ OpenAI 端点测试成功 (HTTP $http_code)${NC}"
        echo "响应: $(echo "$body" | jq -r '.choices[0].message.content' 2>/dev/null || echo "$body" | head -c 200)"
        return 0
    else
        echo -e "${RED}✗ OpenAI 端点测试失败 (HTTP $http_code)${NC}"
        echo "响应: $body"
        return 1
    fi
}

test_anthropic_endpoint() {
    echo -e "${YELLOW}测试 Anthropic Messages API 端点...${NC}"
    echo "端点: https://api.xiaomimimo.com/anthropic/v1/messages"

    local response
    response=$(curl -s -w "\n%{http_code}" \
        -X POST "https://api.xiaomimimo.com/anthropic/v1/messages" \
        -H "x-api-key: ${XIAOMI_MIMO_API_KEY}" \
        -H "anthropic-version: 2023-06-01" \
        -H "Content-Type: application/json" \
        -d '{
            "model": "mimo-v2.5",
            "max_tokens": 100,
            "messages": [{"role": "user", "content": "Say hello in one sentence."}]
        }' 2>&1)

    local http_code
    http_code=$(echo "$response" | tail -n1)
    local body
    body=$(echo "$response" | head -n-1)

    if [[ "$http_code" -eq 200 ]]; then
        echo -e "${GREEN}✓ Anthropic 端点测试成功 (HTTP $http_code)${NC}"
        echo "响应: $(echo "$body" | jq -r '.content[0].text' 2>/dev/null || echo "$body" | head -c 200)"
        return 0
    else
        echo -e "${RED}✗ Anthropic 端点测试失败 (HTTP $http_code)${NC}"
        echo "响应: $body"
        return 1
    fi
}

test_streaming() {
    local endpoint=$1
    local name=$2

    echo -e "${YELLOW}测试 $name 流式响应...${NC}"

    local response
    response=$(curl -s -w "\n%{http_code}" \
        -X POST "$endpoint" \
        -H "Authorization: Bearer ${XIAOMI_MIMO_API_KEY}" \
        -H "Content-Type: application/json" \
        -d '{
            "model": "mimo-v2.5-pro",
            "messages": [{"role": "user", "content": "Say hi"}],
            "max_tokens": 50,
            "stream": true
        }' 2>&1)

    local http_code
    http_code=$(echo "$response" | tail -n1)

    if [[ "$http_code" -eq 200 ]]; then
        echo -e "${GREEN}✓ $name 流式测试成功 (HTTP $http_code)${NC}"
        return 0
    else
        echo -e "${RED}✗ $name 流式测试失败 (HTTP $http_code)${NC}"
        return 1
    fi
}

# 主测试流程
echo "========================================="
echo "MiMo 双端点验收测试"
echo "========================================="
echo ""

# 测试非流式
test_openai_endpoint
echo ""
test_anthropic_endpoint
echo ""

# 测试流式
test_streaming "https://api.xiaomimimo.com/v1/chat/completions" "OpenAI"
echo ""

# 测试通过 mimofan 配置
echo -e "${YELLOW}测试 mimofan 配置解析...${NC}"
echo "配置文件: config/anthropic_config_test.toml"
echo ""
echo "使用方法:"
echo "  1. 复制配置文件: cp config/anthropic_config_test.toml ~/.mimofan/settings.toml"
echo "  2. 设置环境变量: export XIAOMI_MIMO_API_KEY=your_key"
echo "  3. 运行 mimofan: mimofan --profile mimo-anthropic"
echo ""

echo "========================================="
echo -e "${GREEN}验收测试完成${NC}"
echo "========================================="
