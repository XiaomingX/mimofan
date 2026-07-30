#!/usr/bin/env bash
# Mimofan TUI Benchmark Script
# 测试 TUI 二进制的各项能力（自动化模式）

set -euo pipefail

# 配置
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RESULTS_DIR="${SCRIPT_DIR}/results"
TUI_BIN="${SCRIPT_DIR}/../target/release/mimofan"

# 测试配置
TEST_HOME="${MIMOFAN_TEST_HOME:-/tmp/mimofan-benchmark-tui}"
TEST_API_KEY="${MIMOFAN_TEST_API_KEY:-}"
TEST_BASE_URL="${MIMOFAN_TEST_BASE_URL:-}"
TEST_MODEL="${MIMOFAN_TEST_MODEL:-mimo-v2.5}"
TIMEOUT_SECONDS="${MIMOFAN_TEST_TIMEOUT:-60}"

# 颜色输出
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

# 结果文件
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
RESULTS_FILE="${RESULTS_DIR}/tui_benchmark_${TIMESTAMP}.json"
SUMMARY_FILE="${RESULTS_DIR}/tui_summary_${TIMESTAMP}.txt"

# 统计变量
TOTAL_TESTS=0
PASSED_TESTS=0
FAILED_TESTS=0
TOTAL_LATENCY=0
LATENCIES=()

# 函数：记录测试结果
record_result() {
    local test_name="$1"
    local status="$2"
    local latency="$3"
    local output="$4"
    local error="${5:-}"

    TOTAL_TESTS=$((TOTAL_TESTS + 1))

    if [ "$status" = "PASS" ]; then
        PASSED_TESTS=$((PASSED_TESTS + 1))
        echo -e "${GREEN}✓${NC} ${test_name} (${latency}s)"
    else
        FAILED_TESTS=$((FAILED_TESTS + 1))
        echo -e "${RED}✗${NC} ${test_name} (${latency}s)"
        if [ -n "$error" ]; then
            echo -e "  ${RED}Error: ${error}${NC}"
        fi
    fi

    TOTAL_LATENCY=$(echo "$TOTAL_LATENCY + $latency" | bc)
    LATENCIES+=("$latency")

    # 写入 JSON 结果
    cat >> "$RESULTS_FILE" << EOF
  {
    "test_name": "${test_name}",
    "status": "${status}",
    "latency_seconds": ${latency},
    "output_length": ${#output},
    "timestamp": "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  },
EOF
}

# 函数：运行单个 exec 测试
run_exec_test() {
    local test_name="$1"
    local prompt="$2"
    local expected_pattern="${3:-}"

    echo -e "\n${YELLOW}Running: ${test_name}${NC}"

    local start_time=$(date +%s.%N)

    local output
    local exit_code=0
    output=$("${TUI_BIN}" exec "$prompt" 2>&1) || exit_code=$?

    local end_time=$(date +%s.%N)
    local latency=$(echo "$end_time - $start_time" | bc)
    # Ensure latency has leading zero if needed
    if [[ "$latency" == .* ]]; then
        latency="0$latency"
    fi

    # Note: timeout check removed for macOS compatibility

    if [ $exit_code -ne 0 ]; then
        record_result "$test_name" "FAIL" "$latency" "$output" "Exit code: $exit_code"
        return
    fi

    if [ -n "$expected_pattern" ]; then
        if echo "$output" | grep -i "$expected_pattern" > /dev/null 2>&1; then
            record_result "$test_name" "PASS" "$latency" "$output"
        else
            record_result "$test_name" "FAIL" "$latency" "$output" "Expected pattern not found: $expected_pattern"
        fi
    else
        if [ ${#output} -gt 0 ]; then
            record_result "$test_name" "PASS" "$latency" "$output"
        else
            record_result "$test_name" "FAIL" "$latency" "$output" "Empty response"
        fi
    fi
}

# 函数：测试 exec 命令（精简样本，避免重复）
test_exec_commands() {
    echo -e "\n${YELLOW}=== Exec Command Tests ===${NC}"

    run_exec_test "exec_code_generation" \
        "Write a Python function to add two numbers. Only output code." \
        "def"

    run_exec_test "exec_chinese" \
        "用中文回答：中国的首都是哪里？" \
        "北京"

    run_exec_test "exec_instruction_following" \
        "List exactly 3 programming languages. Number them 1-3." \
        ""
}

# 函数：测试 doctor 命令
test_doctor_command() {
    echo -e "\n${YELLOW}=== Doctor Command Tests ===${NC}"

    # doctor 命令有自己的子命令
    local start_time=$(date +%s.%N)
    local output
    local exit_code=0
    output=$("${TUI_BIN}" doctor 2>&1) || exit_code=$?
    local end_time=$(date +%s.%N)
    local latency=$(echo "$end_time - $start_time" | bc)
    # Ensure latency has leading zero if needed
    if [[ "$latency" == .* ]]; then
        latency="0$latency"
    fi

    if [ $exit_code -eq 0 ]; then
        record_result "doctor_direct" "PASS" "$latency" "$output"
    else
        record_result "doctor_direct" "FAIL" "$latency" "$output" "Exit code: $exit_code"
    fi
}

# 函数：测试版本和帮助
test_basic_commands() {
    echo -e "\n${YELLOW}=== Basic Command Tests ===${NC}"

    # 测试版本
    local start_time=$(date +%s.%N)
    local output
    local exit_code=0
    output=$("${TUI_BIN}" --version 2>&1) || exit_code=$?
    local end_time=$(date +%s.%N)
    local latency=$(echo "$end_time - $start_time" | bc)
    # Ensure latency has leading zero if needed
    if [[ "$latency" == .* ]]; then
        latency="0$latency"
    fi

    if [ $exit_code -eq 0 ] && echo "$output" | grep -q "mimofan"; then
        record_result "version" "PASS" "$latency" "$output"
    else
        record_result "version" "FAIL" "$latency" "$output" "Exit code: $exit_code"
    fi

    # 测试帮助
    start_time=$(date +%s.%N)
    output=$("${TUI_BIN}" --help 2>&1) || exit_code=$?
    end_time=$(date +%s.%N)
    latency=$(echo "$end_time - $start_time" | bc)

    if [ $exit_code -eq 0 ] && echo "$output" | grep -q "Usage"; then
        record_result "help" "PASS" "$latency" "$output"
    else
        record_result "help" "FAIL" "$latency" "$output" "Exit code: $exit_code"
    fi
}

# 函数：测试多轮对话（通过 exec 命令模拟）
test_multi_turn() {
    echo -e "\n${YELLOW}=== Multi-turn Conversation Tests ===${NC}"

    # 注意：exec 命令是无状态的，每次调用都是独立的
    # 这里测试的是能否正确处理需要上下文的 prompt

    run_exec_test "multi_turn_context" \
        "Remember the number 42. Now answer: What is the meaning of life, the universe, and everything?" \
        "42"
}

# 函数：测试工具调用（通过 exec 命令）
test_tool_usage() {
    echo -e "\n${YELLOW}=== Tool Usage Tests ===${NC}"

    # 测试 shell 命令执行
    run_exec_test "shell_command" \
        "Run the command 'echo benchmark_test' and show the output" \
        "benchmark_test"
}

# 函数：测试错误与日志捕获记录
test_logging_verification() {
    echo -e "\n${YELLOW}=== Logging & Error Recording Verification ===${NC}"

    # 测试无法识别的子命令并验证错误捕获与日志记录
    local start_time=$(date +%s.%N)
    local output
    local exit_code=0
    output=$("${TUI_BIN}" invalid-subcommand-test 2>&1) || exit_code=$?
    local end_time=$(date +%s.%N)
    local latency=$(echo "$end_time - $start_time" | bc)
    if [[ "$latency" == .* ]]; then latency="0$latency"; fi

    if [ $exit_code -ne 0 ] && (echo "$output" | grep -q -i "error"); then
        record_result "error_logging_capture" "PASS" "$latency" "$output"
    else
        record_result "error_logging_capture" "FAIL" "$latency" "$output" "Error output not detected on invalid command"
    fi
}

# 函数：测试性能
test_performance() {
    echo -e "\n${YELLOW}=== Performance Tests ===${NC}"

    # 测试快速响应
    local start_time=$(date +%s.%N)
    local output
    local exit_code=0
    output=$("${TUI_BIN}" exec "Reply with just 'OK'" 2>&1) || exit_code=$?
    local end_time=$(date +%s.%N)
    local latency=$(echo "$end_time - $start_time" | bc)
    # Ensure latency has leading zero if needed
    if [[ "$latency" == .* ]]; then
        latency="0$latency"
    fi

    if [ $exit_code -eq 0 ] && [ $(echo "$latency < 5" | bc) -eq 1 ]; then
        record_result "fast_response" "PASS" "$latency" "$output"
    else
        record_result "fast_response" "FAIL" "$latency" "$output" "Slow response or error"
    fi
}

# 主函数
main() {
    echo -e "${YELLOW}===================================${NC}"
    echo -e "${YELLOW}  Mimofan TUI Benchmark Suite${NC}"
    echo -e "${YELLOW}===================================${NC}"

    # 检查 TUI 二进制
    if [ ! -f "$TUI_BIN" ]; then
        echo -e "${RED}Error: TUI binary not found at ${TUI_BIN}${NC}"
        echo "Please build first: cargo build --release -p mimofan"
        exit 1
    fi

    # 创建结果目录
    mkdir -p "$RESULTS_DIR"

    # 初始化结果文件
    echo "[" > "$RESULTS_FILE"

    # 设置动态隔离的测试环境（对标 Claude Code / Antigravity 最佳实践）
    TEST_HOME="$(mktemp -d 2>/dev/null || mktemp -d -t 'mimofan-benchmark-XXXXXX')"
    export MIMOFAN_HOME="$TEST_HOME"

    # 注册 EXIT 自动清理钩子
    cleanup() {
        rm -rf "$TEST_HOME"
    }
    trap cleanup EXIT

    # 如果提供了 API 配置，创建配置文件
    if [ -n "$TEST_API_KEY" ]; then
        cat > "$TEST_HOME/config.toml" << EOF
provider = "xiaomi-mimo"
default_text_model = "${TEST_MODEL}"

[providers.xiaomi_mimo]
api_key = "${TEST_API_KEY}"
base_url = "${TEST_BASE_URL}"
EOF
    fi

    # 运行所有测试
    test_basic_commands
    test_exec_commands
    test_doctor_command
    test_multi_turn
    test_tool_usage
    test_logging_verification
    test_performance

    # 完成结果文件（移除最后一个逗号）
    sed -i '' '$ s/,$//' "$RESULTS_FILE"
    echo "]" >> "$RESULTS_FILE"

    # 计算统计
    local success_rate=$(echo "scale=2; $PASSED_TESTS * 100 / $TOTAL_TESTS" | bc)
    local avg_latency=$(echo "scale=2; $TOTAL_LATENCY / $TOTAL_TESTS" | bc)

    # 排序延迟计算 P50 和 P95
    IFS=$'\n' sorted_latencies=($(sort -n <<<"${LATENCIES[*]}"))
    unset IFS
    local p50_index=$(echo "$TOTAL_TESTS / 2" | bc)
    local p95_index=$(echo "$TOTAL_TESTS * 95 / 100" | bc)
    local latency_p50=${sorted_latencies[$p50_index]:-0}
    local latency_p95=${sorted_latencies[$p95_index]:-0}

    # 生成摘要
    cat > "$SUMMARY_FILE" << EOF
====================================
  TUI Benchmark Summary
====================================
Date: $(date)
Model: ${TEST_MODEL}
Total Tests: ${TOTAL_TESTS}
Passed: ${PASSED_TESTS}
Failed: ${FAILED_TESTS}
Success Rate: ${success_rate}%

Latency Statistics:
  Average: ${avg_latency}s
  P50: ${latency_p50}s
  P95: ${latency_p95}s

Results File: ${RESULTS_FILE}
====================================
EOF

    # 输出摘要
    echo -e "\n${YELLOW}===================================${NC}"
    echo -e "${YELLOW}  Benchmark Complete${NC}"
    echo -e "${YELLOW}===================================${NC}"
    cat "$SUMMARY_FILE"

    # 返回失败测试数作为退出码
    exit $FAILED_TESTS
}

main "$@"
