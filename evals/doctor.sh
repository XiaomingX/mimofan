#!/bin/bash
# mimofan 配置诊断脚本
# 用法: ./doctor.sh

set -e

echo "=== mimofan 配置诊断 ==="
echo ""

# 1. 检查二进制是否存在
echo "--- 检查二进制 ---"
if command -v mimofan &> /dev/null; then
    MIMOFAN_PATH=$(command -v mimofan)
    echo "✅ mimofan 已安装: $MIMOFAN_PATH"
    mimofan --version 2>/dev/null || echo "⚠️  mimofan --version 失败"
elif [ -f "./target/release/mimofan" ]; then
    echo "✅ mimofan 已构建: ./target/release/mimofan"
    ./target/release/mimofan --version 2>/dev/null || echo "⚠️  version 命令失败"
elif [ -f "./target/debug/mimofan" ]; then
    echo "⚠️  仅找到 debug 版本: ./target/debug/mimofan"
    ./target/debug/mimofan --version 2>/dev/null || echo "⚠️  version 命令失败"
else
    echo "❌ mimofan 未安装，请先运行 ./build.sh"
fi

echo ""

# 2. 检查配置目录
echo "--- 检查配置目录 ---"
MIMOFAN_HOME="${MIMOFAN_HOME:-$HOME/.mimofan}"
if [ -d "$MIMOFAN_HOME" ]; then
    echo "✅ 配置目录存在: $MIMOFAN_HOME"
else
    echo "⚠️  配置目录不存在: $MIMOFAN_HOME"
    echo "   运行以下命令创建:"
    echo "   mkdir -p $MIMOFAN_HOME"
    echo "   cp config.example.toml $MIMOFAN_HOME/config.toml"
fi

# 3. 检查配置文件
echo ""
echo "--- 检查配置文件 ---"
if [ -f "$MIMOFAN_HOME/config.toml" ]; then
    echo "✅ 配置文件存在: $MIMOFAN_HOME/config.toml"

    # 检查 provider
    PROVIDER=$(grep -E "^provider\s*=" "$MIMOFAN_HOME/config.toml" 2>/dev/null | head -1 | sed 's/.*=\s*//' | tr -d ' "')
    if [ -n "$PROVIDER" ]; then
        echo "   provider: $PROVIDER"
    fi

    # 检查 api_key
    if grep -q "api_key\s*=" "$MIMOFAN_HOME/config.toml" 2>/dev/null; then
        API_KEY=$(grep "api_key\s*=" "$MIMOFAN_HOME/config.toml" | head -1 | sed 's/.*=\s*//' | tr -d ' "')
        if [ -n "$API_KEY" ] && [ "$API_KEY" != "YOUR_API_KEY" ] && [ "$API_KEY" != "YOUR_MIMO_API_KEY" ]; then
            echo "   api_key: ✅ 已配置"
        else
            echo "   api_key: ⚠️  未配置或为占位符"
        fi
    else
        echo "   api_key: ⚠️  未找到"
    fi
else
    echo "⚠️  配置文件不存在: $MIMOFAN_HOME/config.toml"
fi

echo ""

# 4. 检查环境变量
echo "--- 检查环境变量 ---"
if [ -n "$MIMO_API_KEY" ]; then
    echo "✅ MIMO_API_KEY 已设置"
elif [ -n "$DEEPSEEK_API_KEY" ]; then
    echo "✅ DEEPSEEK_API_KEY 已设置"
elif [ -n "$OPENAI_API_KEY" ]; then
    echo "✅ OPENAI_API_KEY 已设置"
else
    echo "⚠️  未检测到 LLM API key 环境变量"
fi

echo ""

# 5. 运行 mimofan doctor
echo "--- mimofan doctor ---"
if command -v mimofan &> /dev/null; then
    mimofan doctor 2>/dev/null || echo "⚠️  doctor 命令执行失败"
elif [ -f "./target/release/mimofan" ]; then
    ./target/release/mimofan doctor 2>/dev/null || echo "⚠️  doctor 命令执行失败"
elif [ -f "./target/debug/mimofan" ]; then
    ./target/debug/mimofan doctor 2>/dev/null || echo "⚠️  doctor 命令执行失败"
fi

echo ""
echo "=== 诊断完成 ==="
