#!/bin/bash
# mimofan 集成测试脚本
# 测试 mimofan 二进制能否正确调用两种 API 端点

set -e

# 加载配置
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/config.env"

CONFIG_DIR="${HOME}/.mimofan"
CONFIG_FILE="${CONFIG_DIR}/config.toml"

echo "=== mimofan 集成测试 ==="
echo ""

# 检查二进制是否存在
if [ ! -f "$MIMOFAN_BIN" ]; then
  echo "❌ 错误: 二进制文件不存在: $MIMOFAN_BIN"
  echo "请先运行: cargo build --release -p mimofan"
  exit 1
fi

# 备份现有配置
if [ -f "$CONFIG_FILE" ]; then
  echo "备份现有配置: ${CONFIG_FILE}.bak"
  cp "$CONFIG_FILE" "${CONFIG_FILE}.bak"
fi

# 测试 1: Anthropic Messages API 配置
echo "测试 1: Anthropic Messages API"
echo "  配置: provider=custom, base_url=${ANTHROPIC_BASE_URL}"
echo ""

cat > "$CONFIG_FILE" << EOF
[default]
provider = "custom"
api_key = "${API_KEY}"
base_url = "${ANTHROPIC_BASE_URL}"
default_text_model = "${MODEL}"
EOF

# 测试 mimofan exec
echo "执行 mimofan exec..."
if timeout ${TIMEOUT_SECONDS} "$MIMOFAN_BIN" exec "Say hello in 5 words" 2>&1; then
  echo "✅ Anthropic Messages API 测试成功"
else
  echo "❌ Anthropic Messages API 测试失败"
fi
echo ""

# 测试 2: OpenAI Chat Completions 配置
echo "测试 2: OpenAI Chat Completions API"
echo "  配置: provider=custom, base_url=${OPENAI_BASE_URL}"
echo ""

cat > "$CONFIG_FILE" << EOF
[default]
provider = "custom"
api_key = "${API_KEY}"
base_url = "${OPENAI_BASE_URL}"
default_text_model = "${MODEL}"
EOF

# 测试 mimofan exec
echo "执行 mimofan exec..."
if timeout ${TIMEOUT_SECONDS} "$MIMOFAN_BIN" exec "Say hello in 5 words" 2>&1; then
  echo "✅ OpenAI Chat Completions API 测试成功"
else
  echo "❌ OpenAI Chat Completions API 测试失败"
fi
echo ""

# 恢复备份
if [ -f "${CONFIG_FILE}.bak" ]; then
  echo "恢复备份配置"
  mv "${CONFIG_FILE}.bak" "$CONFIG_FILE"
fi

echo "=== 测试完成 ==="
