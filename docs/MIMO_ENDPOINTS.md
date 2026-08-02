# MiMo 双端点支持

## 概述

Mimofan 现在支持 Xiaomi MiMo 的两个 API 端点：

1. **OpenAI Chat Completions 兼容端点**
   - 端点: `https://api.xiaomimimo.com/v1`
   - 协议: OpenAI Chat Completions API
   - Provider: `xiaomi-mimo` (默认)

2. **Anthropic Messages API 兼容端点**
   - 端点: `https://api.xiaomimimo.com/anthropic`
   - 协议: Anthropic Messages API
   - Provider: `anthropic`

## 配置方法

### 方法 1: 使用 Profile

在 `~/.mimofan/settings.toml` 中配置：

```toml
# OpenAI 兼容端点 (默认)
[profiles.mimo]
provider = "xiaomi-mimo"
api_key = "YOUR_XIAOMI_KEY"
default_text_model = "mimo-v2.5-pro"

# Anthropic 兼容端点
[profiles.mimo-anthropic]
provider = "anthropic"
api_key = "YOUR_XIAOMI_KEY"
default_text_model = "mimo-v2.5"
```

使用方法：

```bash
# 使用 OpenAI 端点
mimofan --profile mimo

# 使用 Anthropic 端点
mimofan --profile mimo-anthropic
```

### 方法 2: 直接配置

在 `~/.mimofan/settings.toml` 中：

```toml
# OpenAI 端点
provider = "xiaomi-mimo"
api_key = "YOUR_XIAOMI_KEY"
base_url = "https://api.xiaomimimo.com/v1"
default_text_model = "mimo-v2.5-pro"

# 或者 Anthropic 端点
provider = "anthropic"
api_key = "YOUR_XIAOMI_KEY"
base_url = "https://api.xiaomimimo.com/anthropic"
default_text_model = "mimo-v2.5"
```

## 验收测试

### 运行验收脚本

```bash
# 设置 API Key
export XIAOMI_MIMO_API_KEY=your_api_key

# 运行验收测试
./evals/test_mimo_endpoints.sh
```

### 手动测试

#### 测试 OpenAI 端点

```bash
curl -X POST https://api.xiaomimimo.com/v1/chat/completions \
  -H "Authorization: Bearer $XIAOMI_MIMO_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "mimo-v2.5-pro",
    "messages": [{"role": "user", "content": "Hello"}],
    "max_tokens": 100
  }'
```

#### 测试 Anthropic 端点

```bash
curl -X POST https://api.xiaomimimo.com/anthropic/v1/messages \
  -H "x-api-key: $XIAOMI_MIMO_API_KEY" \
  -H "anthropic-version: 2023-06-01" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "mimo-v2.5",
    "max_tokens": 100,
    "messages": [{"role": "user", "content": "Hello"}]
  }'
```

## 技术实现

### 代码变更

1. **ProviderKind 枚举** (`crates/config/src/provider_kind.rs`)
   - 添加 `Anthropic` 变体
   - 更新 `ALL` 常量和 `as_str` 方法

2. **Provider 默认配置** (`crates/config/src/provider_defaults.rs`)
   - 添加 `XIAOMI_MIMO_ANTHROPIC_BASE_URL` 常量

3. **Provider 实现** (`crates/config/src/provider.rs`)
   - 添加 `AnthropicProvider` 结构体
   - 实现 `Provider` trait，返回 `WireFormat::AnthropicMessages`

4. **路由解析** (`crates/config/src/route/resolver.rs`)
   - 更新 `classify` 函数处理 Anthropic provider

5. **配置支持** (`crates/config/src/lib.rs`)
   - 添加 `anthropic` 字段到 `ProvidersToml`
   - 更新 `for_provider` 和 `for_provider_mut` 方法

6. **TUI 配置** (`crates/tui/src/config.rs`)
   - 添加 `Anthropic` 到 `ApiProvider` 枚举
   - 更新所有 match 语句处理 Anthropic provider

7. **客户端** (`crates/tui/src/client.rs`)
   - 更新推理模式处理逻辑支持 Anthropic 协议

### 协议支持

- **OpenAI 端点**: 使用 `/v1/chat/completions` 接口
- **Anthropic 端点**: 使用 `/v1/messages` 接口

两个端点都支持：
- 流式和非流式响应
- 工具调用
- 推理模式 (reasoning_effort)
- 缓存控制

## 相关文件

- `config/config.example.toml` - 配置示例
- `config/anthropic_config_test.toml` - Anthropic 配置测试
- `evals/test_mimo_endpoints.sh` - 验收测试脚本
- `evals/config.toml` - 模型对比配置
- `docs/MIMO_ENDPOINTS.md` - 本文档
