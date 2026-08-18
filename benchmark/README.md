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

- **API 密钥**: 通过环境变量 `MIMOFAN_TEST_API_KEY` 注入（CI secret），仓库不内置密钥
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

## 长程任务 / 长期记忆评测样本集（`long_horizon/`）

从公开评测集（SciCode / Terminal-Bench / HLE）筛选的「长程任务 / 长期记忆 / 一致性」代表性样本，详见 `long_horizon/README.md`。

**目录内容**：
- `long_horizon/fetch_data.py` — 抽取外部样本到 `long_horizon/samples/`
- `long_horizon/samples/*.json` — SciCode 长程多步 / Terminal-Bench 端到端 / HLE 多步推理子集
- `long_horizon/long_horizon_mece.json` — 长程执行视角补充条目（MECE 风格，独立文件，锚定 D06 真实符号 `task_manager`/`goal_loop`/`loop_guard`）
- `long_horizon/run_eval.py` — 长程任务端到端评分 harness（三维：子步完成度 / 跨步一致性 / 防卡死）

**运行**：
```bash
python3 benchmark/long_horizon/fetch_data.py              # 拉取外部样本
python3 benchmark/long_horizon/run_eval.py --selftest     # 校验评分逻辑（无需模型）
python3 benchmark/long_horizon/run_eval.py --limit 5      # 真模型端到端评分（需 ANTHROPIC_* 环境变量）
```

**说明**：MECE 骨架已冻结（不得在 D06 簇内超配额补充），故长程视角条目以独立文件 `long_horizon_mece.json` 承载，不写入 `agentbench/samples/mece_1000/`。长期记忆（跨会话召回）维度由既有 `longmemeval_harness.py` + D05（100 条）覆盖，本目录不重复造样本。
