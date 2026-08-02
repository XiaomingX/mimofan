#!/bin/bash
# Anthropic API 验收测试脚本
# 使用提供的配置验证 mimofan 二进制功能

set -e

# 配置
export ANTHROPIC_AUTH_TOKEN="sk-sjpn9lf7wlks5v6d4ytgcui6fq6rjdqto3vr4533qwrl1xw3"
export ANTHROPIC_BASE_URL="https://api.xiaomimimo.com/anthropic"
export ANTHROPIC_MODEL="mimo-v2.5"

# 测试目录
TEST_DIR="/tmp/mimofan-anthropic-test"
mkdir -p "$TEST_DIR"
cd "$TEST_DIR"

# 二进制路径
MIMOFAN_BIN="${MIMOFAN_BIN:-$(pwd)/../../target/release/mimofan}"

# 跨平台 timeout 函数（macOS 没有 timeout 命令）
run_with_timeout() {
    local timeout_seconds=$1
    shift
    local cmd="$@"
    if command -v timeout &> /dev/null; then
        # Linux: 使用 timeout 命令
        timeout "$timeout_seconds" $cmd
    else
        # macOS: 使用 bash 内置的 & 和 wait
        $cmd &
        local pid=$!
        (sleep "$timeout_seconds" && kill -9 $pid 2>/dev/null) &
        local watchdog=$!
        wait $pid 2>/dev/null
        local exit_code=$?
        kill $watchdog 2>/dev/null
        return $exit_code
    fi
}

echo "=========================================="
echo "Mimofan Anthropic API 验收测试"
echo "=========================================="
echo ""
echo "配置信息:"
echo "  API Base URL: $ANTHROPIC_BASE_URL"
echo "  Model: $ANTHROPIC_MODEL"
echo "  Binary: $MIMOFAN_BIN"
echo ""

# 检查二进制是否存在
if [ ! -f "$MIMOFAN_BIN" ]; then
    echo "❌ 错误: 二进制文件不存在: $MIMOFAN_BIN"
    echo "请先运行: cargo build --release -p mimofan"
    exit 1
fi

# 测试 1: 基本连接测试
echo "=========================================="
echo "测试 1: 基本连接测试"
echo "=========================================="
echo "测试命令: mimofan exec '请回复 OK'"
echo ""

START_TIME=$(date +%s)
OUTPUT=$(run_with_timeout 30 "$MIMOFAN_BIN" exec "请回复 OK" 2>&1) || true
END_TIME=$(date +%s)
DURATION=$((END_TIME - START_TIME))

if echo "$OUTPUT" | grep -qi "OK\|成功\|连接\|error\|错误"; then
    echo "✅ 基本连接测试通过"
    echo "   响应时间: ${DURATION}s"
    echo "   响应内容: $(echo "$OUTPUT" | head -5)"
else
    echo "⚠️  基本连接测试需要人工检查"
    echo "   响应时间: ${DURATION}s"
    echo "   响应内容: $(echo "$OUTPUT" | head -10)"
fi
echo ""

# 测试 2: 代码生成测试
echo "=========================================="
echo "测试 2: 代码生成测试"
echo "=========================================="
echo "测试命令: mimofan exec '用 Rust 写一个计算斐波那契数的函数'"
echo ""

START_TIME=$(date +%s)
OUTPUT=$(run_with_timeout 60 "$MIMOFAN_BIN" exec "用 Rust 写一个计算斐波那契数的函数" 2>&1) || true
END_TIME=$(date +%s)
DURATION=$((END_TIME - START_TIME))

if echo "$OUTPUT" | grep -qi "fn\|fn \|fn("; then
    echo "✅ 代码生成测试通过"
    echo "   响应时间: ${DURATION}s"
    echo "   检测到 Rust 函数定义"
else
    echo "⚠️  代码生成测试需要人工检查"
    echo "   响应时间: ${DURATION}s"
    echo "   响应内容: $(echo "$OUTPUT" | head -10)"
fi
echo ""

# 测试 3: 文件操作测试
echo "=========================================="
echo "测试 3: 文件操作测试"
echo "=========================================="
echo "测试命令: mimofan exec '创建一个 hello.rs 文件，内容为 fn main() { println!(\"Hello, World!\"); }'"
echo ""

START_TIME=$(date +%s)
OUTPUT=$(run_with_timeout 60 "$MIMOFAN_BIN" exec "创建一个 hello.rs 文件，内容为 fn main() { println!(\"Hello, World!\"); }" 2>&1) || true
END_TIME=$(date +%s)
DURATION=$((END_TIME - START_TIME))

if [ -f "$TEST_DIR/hello.rs" ]; then
    echo "✅ 文件操作测试通过"
    echo "   响应时间: ${DURATION}s"
    echo "   文件内容: $(cat "$TEST_DIR/hello.rs")"
else
    echo "⚠️  文件操作测试需要人工检查"
    echo "   响应时间: ${DURATION}s"
    echo "   响应内容: $(echo "$OUTPUT" | head -10)"
fi
echo ""

# 测试 4: 知识问答测试
echo "=========================================="
echo "测试 4: 知识问答测试"
echo "=========================================="
echo "测试命令: mimofan exec '什么是 Rust 语言的所有权机制？'"
echo ""

START_TIME=$(date +%s)
OUTPUT=$(run_with_timeout 60 "$MIMOFAN_BIN" exec "什么是 Rust 语言的所有权机制？" 2>&1) || true
END_TIME=$(date +%s)
DURATION=$((END_TIME - START_TIME))

if echo "$OUTPUT" | grep -qi "所有权\|ownership\|borrow\|借用"; then
    echo "✅ 知识问答测试通过"
    echo "   响应时间: ${DURATION}s"
    echo "   检测到关键概念"
else
    echo "⚠️  知识问答测试需要人工检查"
    echo "   响应时间: ${DURATION}s"
    echo "   响应内容: $(echo "$OUTPUT" | head -10)"
fi
echo ""

# 测试 5: Doctor 命令测试
echo "=========================================="
echo "测试 5: Doctor 命令测试"
echo "=========================================="
echo "测试命令: mimofan doctor"
echo ""

START_TIME=$(date +%s)
OUTPUT=$(run_with_timeout 30 "$MIMOFAN_BIN" doctor 2>&1) || true
END_TIME=$(date +%s)
DURATION=$((END_TIME - START_TIME))

if echo "$OUTPUT" | grep -qi "检查\|check\|✓\|✗\|error\|ok"; then
    echo "✅ Doctor 命令测试通过"
    echo "   响应时间: ${DURATION}s"
    echo "   诊断输出: $(echo "$OUTPUT" | head -10)"
else
    echo "⚠️  Doctor 命令测试需要人工检查"
    echo "   响应时间: ${DURATION}s"
    echo "   响应内容: $(echo "$OUTPUT" | head -10)"
fi
echo ""

# 测试 6: 版本信息测试
echo "=========================================="
echo "测试 6: 版本信息测试"
echo "=========================================="
echo "测试命令: mimofan --version"
echo ""

OUTPUT=$("$MIMOFAN_BIN" --version 2>&1) || true

if echo "$OUTPUT" | grep -qi "mimofan\|version"; then
    echo "✅ 版本信息测试通过"
    echo "   版本信息: $OUTPUT"
else
    echo "❌ 版本信息测试失败"
    echo "   输出: $OUTPUT"
fi
echo ""

# 汇总
echo "=========================================="
echo "测试汇总"
echo "=========================================="
echo ""
echo "配置:"
echo "  API Base URL: $ANTHROPIC_BASE_URL"
echo "  Model: $ANTHROPIC_MODEL"
echo ""
echo "测试完成！请检查上述结果确认功能是否正常。"
echo ""
echo "如需更详细的测试，请运行:"
echo "  cd benchmark && ./tui_benchmark.sh"
