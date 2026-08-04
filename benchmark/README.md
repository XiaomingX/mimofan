# Benchmark 测试脚本

## 概述

本目录包含用于验收测试的脚本，确保每次发布不影响核心功能。

## 测试脚本

### API Provider 基础能力验收测试

**文件**: `api_providers_test.sh`

**用途**: 验收 OpenAI 和 Anthropic 模式的基础调用能力

**测试内容**:
- OpenAI 模式 - 直接 API 测试
- OpenAI 模式 - 通过 mimofan 调用
- Anthropic 模式 - 直接 API 测试
- Anthropic 模式 - 通过 mimofan 调用
- 功能测试 - 数学计算

**使用方法**:

```bash
# 运行测试
./benchmark/api_providers_test.sh

# 或者从项目根目录运行
cd /path/to/agent-mimofan
./benchmark/api_providers_test.sh
```

## 配置要求

测试脚本使用以下配置：

- **API 密钥**: `sk-sisyqk4mvbhk13uyiq9lsf4jtr3a0vji2dijeu5ujm71v7im`
- **OpenAI 网关**: `https://api.xiaomimimo.com/v1`
- **Anthropic 网关**: `https://api.xiaomimimo.com/anthropic`
- **模型**: `mimo-v2.5`

配置文件位置：`~/.mimofan/config.toml`

## 发布前检查清单

每次发布前，请运行以下命令：

```bash
# 1. 运行 API Provider 验收测试
./benchmark/api_providers_test.sh

# 2. 运行完整工作区测试
cargo test --workspace

# 3. 构建 release 版本
cargo build --release -p mimofan
```
