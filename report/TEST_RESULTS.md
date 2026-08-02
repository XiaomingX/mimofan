# API 路由验收测试报告

**测试日期**: 2026-08-02
**测试环境**: macOS Darwin 24.6.0
**API Key**: `sk-shvcl8e7xx1nj1i4kr8u05wcglhmzwbkvo501ln7oyinfwxx`

---

## 测试概述

本次验收测试验证了 mimofan 项目对两种 API 端点的支持：
1. **Anthropic Messages API** (`/anthropic/v1/messages`)
2. **OpenAI Chat Completions API** (`/v1/chat/completions`)

---

## 测试结果

### ✅ 测试 1: Anthropic Messages API

| 项目 | 结果 |
|------|------|
| 端点 | `https://api.xiaomimimo.com/anthropic/v1/messages` |
| HTTP 状态码 | 200 |
| 响应格式 | Anthropic Messages API 格式 |
| 模型 | `mimo-v2.5` |
| 状态 | ✅ 成功 |

**响应示例**:
```json
{
  "id": "3c9cfe22-c5ec-4401-9eb6-6019f07e38f1_...",
  "type": "message",
  "role": "assistant",
  "model": "mimo-v2.5",
  "stop_reason": "max_tokens",
  "content": [{"type": "thinking", "thinking": "..."}]
}
```

### ✅ 测试 2: OpenAI Chat Completions API

| 项目 | 结果 |
|------|------|
| 端点 | `https://api.xiaomimimo.com/v1/chat/completions` |
| HTTP 状态码 | 200 |
| 响应格式 | OpenAI Chat Completions 格式 |
| 模型 | `mimo-v2.5` |
| 状态 | ✅ 成功 |

**响应示例**:
```json
{
  "id": "5f5ce2ca-8503-4cf3-834a-524d06a34f62_...",
  "choices": [{
    "finish_reason": "stop",
    "index": 0,
    "message": {
      "content": "Hello there, nice to meet you!",
      "role": "assistant"
    }
  }]
}
```

---

## 代码修改

### 1. API 路由逻辑修复

**文件**: `crates/tui/src/client.rs:864-878`

**修改内容**: 更新 `api_provider_uses_anthropic_messages` 函数，支持 `Custom` provider

```rust
fn api_provider_uses_anthropic_messages(api_provider: ApiProvider, base_url: &str) -> bool {
    // XiaomiMiMo serves two protocol dialects from different paths. When the
    // configured base URL ends in `/anthropic` (e.g.
    // `https://api.xiaomimimo.com/anthropic`) the gateway expects the native
    // Anthropic Messages wire format. Other base URLs (token-plan pay-as-you-
    // go, local proxies, custom gateways) keep using the Responses dialect.
    //
    // For Custom providers, also detect Anthropic Messages API if the base_url
    // ends with `/anthropic`. This enables custom providers to work with
    // Anthropic-compatible API endpoints.
    if matches!(api_provider, ApiProvider::XiaomiMimo | ApiProvider::Custom) {
        return base_url.trim_end_matches('/').ends_with("/anthropic");
    }
    false
}
```

### 2. 单元测试更新

**文件**: `crates/tui/src/client.rs:1774-1795`

**新增测试用例**:
- `custom_provider_with_anthropic_url_uses_messages_api`
- `custom_provider_with_v1_url_uses_chat_completions`
- `xiaomi_mimo_with_v1_url_uses_chat_completions`

### 3. README.md 文档修复

**文件**: `README.md:126-233`

**修改内容**:
- 更新多模型支持表，修正 Anthropic 配置说明
- 添加 Anthropic Messages API 配置示例
- 添加 OpenAI Chat Completions 配置示例

---

## Benchmark 脚本优化

### 1. 统一配置文件

**文件**: `benchmark/config.env`

**功能**: 统一管理所有测试脚本的配置参数

### 2. API 端点验收测试

**文件**: `benchmark/test_api_endpoints.sh`

**功能**:
- 测试 Anthropic Messages API 端点
- 测试 OpenAI Chat Completions API 端点
- 生成清晰的测试报告

### 3. mimofan 集成测试

**文件**: `benchmark/test_mimofan_integration.sh`

**功能**:
- 使用 `mimofan exec` 命令测试两种 API 配置
- 验证配置文件能正确加载
- 生成测试报告

### 4. 一键运行脚本

**文件**: `benchmark/run_all.sh`

**功能**: 一键运行所有测试，包括 API 端点测试、集成测试和基准测试

---

## 验收结论

### ✅ 通过的验收项

1. **API 路由逻辑** - 两种 API 端点都能正确路由
2. **Anthropic Messages API** - `/anthropic/v1/messages` 端点正常工作
3. **OpenAI Chat Completions API** - `/v1/chat/completions` 端点正常工作
4. **单元测试** - 所有测试通过
5. **Release 构建** - 构建成功
6. **文档** - README.md 配置示例正确
7. **Benchmark 脚本** - 优化后的脚本正常工作

### 📝 备注

1. **模型支持**: `api.xiaomimimo.com/anthropic` 网关支持 `mimo-v2.5` 模型，但不支持 `claude-sonnet-4-20250514` 模型
2. **协议自动检测**: mimofan 通过 `base_url` 是否以 `/anthropic` 结尾来自动选择协议
3. **Provider 类型**: 使用 `provider = "custom"` 配合不同的 `base_url` 来支持两种 API

---

## 快速验收命令

```bash
# 一键运行所有测试
./benchmark/run_all.sh

# 单独运行 API 端点测试
./benchmark/test_api_endpoints.sh

# 单独运行集成测试
./benchmark/test_mimofan_integration.sh
```

---

## 后续建议

1. **模型文档**: 在 README.md 中明确说明 `api.xiaomimimo.com/anthropic` 网关支持的模型列表
2. **错误处理**: 考虑在模型不支持时提供更友好的错误信息
3. **CI 集成**: 将 benchmark 脚本集成到 CI/CD 流程中
