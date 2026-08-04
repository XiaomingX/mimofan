#!/bin/bash
# =============================================================================
# API Provider 基础能力验收测试
# =============================================================================
# 用途：每次发布前验收 OpenAI 和 Anthropic 模式的基础调用能力
# 使用：./benchmark/api_providers_test.sh
# =============================================================================

set -e

# 颜色定义
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# 测试配置
API_KEY="sk-sfl2f69ak7vrf538yq093akm8ngh149cf489eqmvfam3ndhi"
OPENAI_BASE_URL="https://api.xiaomimimo.com/v1"
ANTHROPIC_BASE_URL="https://api.xiaomimimo.com/anthropic"
MODEL="mimo-v2.5"

# 测试提示词
TEST_PROMPT="Hello, please respond with just 'OK'"

# 统计变量
TOTAL_TESTS=0
PASSED_TESTS=0
FAILED_TESTS=0

# =============================================================================
# 辅助函数
# =============================================================================

print_header() {
    echo ""
    echo "=========================================="
    echo "  $1"
    echo "=========================================="
}

print_test_result() {
    local test_name="$1"
    local status="$2"
    local response="$3"

    TOTAL_TESTS=$((TOTAL_TESTS + 1))

    if [ "$status" = "PASS" ]; then
        echo -e "${GREEN}✓ PASS${NC}: $test_name"
        PASSED_TESTS=$((PASSED_TESTS + 1))
    else
        echo -e "${RED}✗ FAIL${NC}: $test_name"
        echo "  Response: $response"
        FAILED_TESTS=$((FAILED_TESTS + 1))
    fi
}

# =============================================================================
# OpenAI 模式测试
# =============================================================================

test_openai_direct_api() {
    print_header "OpenAI 模式 - 直接 API 测试"

    local response=$(curl -s -w "\n%{http_code}" -X POST "${OPENAI_BASE_URL}/chat/completions" \
        -H "Content-Type: application/json" \
        -H "Authorization: Bearer ${API_KEY}" \
        -d "{
            \"model\": \"${MODEL}\",
            \"max_tokens\": 100,
            \"messages\": [
                {
                    \"role\": \"user\",
                    \"content\": \"${TEST_PROMPT}\"
                }
            ]
        }" 2>&1)

    local http_code=$(echo "$response" | tail -n1)
    local body=$(echo "$response" | sed '$d')

    if [ "$http_code" = "200" ] && echo "$body" | grep -q '"content":"OK"'; then
        print_test_result "OpenAI Direct API" "PASS"
    else
        print_test_result "OpenAI Direct API" "FAIL" "HTTP $http_code"
    fi
}

test_openai_via_mimofan() {
    print_header "OpenAI 模式 - 通过 mimofan 调用"

    local response=$(./target/release/mimofan exec --provider openai "${TEST_PROMPT}" 2>&1)

    if echo "$response" | grep -q "OK"; then
        print_test_result "OpenAI via mimofan" "PASS"
    else
        print_test_result "OpenAI via mimofan" "FAIL" "$response"
    fi
}

# =============================================================================
# Anthropic 模式测试
# =============================================================================

test_anthropic_direct_api() {
    print_header "Anthropic 模式 - 直接 API 测试"

    local response=$(curl -s -w "\n%{http_code}" -X POST "${ANTHROPIC_BASE_URL}/v1/messages" \
        -H "Content-Type: application/json" \
        -H "x-api-key: ${API_KEY}" \
        -H "anthropic-version: 2023-06-01" \
        -d "{
            \"model\": \"${MODEL}\",
            \"max_tokens\": 100,
            \"messages\": [
                {
                    \"role\": \"user\",
                    \"content\": \"${TEST_PROMPT}\"
                }
            ]
        }" 2>&1)

    local http_code=$(echo "$response" | tail -n1)
    local body=$(echo "$response" | sed '$d')

    if [ "$http_code" = "200" ] && echo "$body" | grep -q '"text":"OK"'; then
        print_test_result "Anthropic Direct API" "PASS"
    else
        print_test_result "Anthropic Direct API" "FAIL" "HTTP $http_code"
    fi
}

test_anthropic_via_mimofan() {
    print_header "Anthropic 模式 - 通过 mimofan 调用"

    local response=$(./target/release/mimofan exec --provider xiaomi-mimo "${TEST_PROMPT}" 2>&1)

    if echo "$response" | grep -q "OK"; then
        print_test_result "Anthropic via mimofan" "PASS"
    else
        print_test_result "Anthropic via mimofan" "FAIL" "$response"
    fi
}

# =============================================================================
# 功能测试
# =============================================================================

test_math_capability() {
    print_header "功能测试 - 数学计算"

    local response=$(./target/release/mimofan exec --provider openai "What is 2+2? Please respond with just the number." 2>&1)

    if echo "$response" | grep -q "4"; then
        print_test_result "Math capability" "PASS"
    else
        print_test_result "Math capability" "FAIL" "$response"
    fi
}

# =============================================================================
# 主测试流程
# =============================================================================

main() {
    echo ""
    echo "╔════════════════════════════════════════════════════════════╗"
    echo "║        API Provider 基础能力验收测试                       ║"
    echo "║        mimofan $(./target/release/mimofan --version 2>/dev/null | head -1)                     ║"
    echo "╚════════════════════════════════════════════════════════════╝"
    echo ""

    # OpenAI 模式测试
    test_openai_direct_api
    test_openai_via_mimofan

    # Anthropic 模式测试
    test_anthropic_direct_api
    test_anthropic_via_mimofan

    # 功能测试
    test_math_capability

    # 汇总结果
    echo ""
    echo "=========================================="
    echo "  测试结果汇总"
    echo "=========================================="
    echo ""
    echo "总测试数: $TOTAL_TESTS"
    echo -e "通过: ${GREEN}$PASSED_TESTS${NC}"
    echo -e "失败: ${RED}$FAILED_TESTS${NC}"
    echo ""

    if [ $FAILED_TESTS -eq 0 ]; then
        echo -e "${GREEN}✓ 所有测试通过！${NC}"
        exit 0
    else
        echo -e "${RED}✗ 有 $FAILED_TESTS 个测试失败${NC}"
        exit 1
    fi
}

# 运行主测试流程
main
