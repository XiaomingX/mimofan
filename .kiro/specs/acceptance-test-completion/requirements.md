# 验收测试覆盖率补全 - 需求文档

## 简介

将 mimofan 项目的基础功能验收覆盖率从 87% 提升至理论上限。遵循奥卡姆剃刀原则（最简方案）和 MECE 原则（互不重叠、完全穷尽），不实现冗余代码，不考虑向下兼容。

## 覆盖缺口分析

| 类别 | 已覆盖 | 总计 | 缺口 | 可测试性 |
|------|--------|------|------|----------|
| 内置工具 | 41 | 42 | 1 | ❌ 内部辅助函数（shell_output），按奥卡姆剃刀跳过 |
| 命令系统 | 48 | 61 | 13 | ✅ 可通过 `mimofan exec` 测试 |
| 子系统集成 | 7 | 9 | 2 | ⚠️ 部分可测试 |

**理论上限：126/127 = 99%**（跳过 1 个内部辅助函数）

## 需求

### 1. TUI 命令验收测试（13 项）

**用户故事：** 作为开发者，我需要验证所有 TUI 交互式命令可被识别和执行，以便确认命令注册完整性。

**验收标准：**

- WHEN 运行 `mimofan exec -c "/agent --help"` THEN 系统 SHALL 返回帮助信息或执行结果
- WHEN 运行 `mimofan exec -c "/anchor --help"` THEN 系统 SHALL 返回帮助信息或执行结果
- WHEN 运行 `mimofan exec -c "/auto --help"` THEN 系统 SHALL 返回帮助信息或执行结果
- WHEN 运行 `mimofan exec -c "/clear"` THEN 系统 SHALL 执行清空操作
- WHEN 运行 `mimofan exec -c "/exit"` THEN 系统 SHALL 执行退出操作
- WHEN 运行 `mimofan exec -c "/fast --help"` THEN 系统 SHALL 返回帮助信息或执行结果
- WHEN 运行 `mimofan exec -c "/feedback --help"` THEN 系统 SHALL 返回帮助信息或执行结果
- WHEN 运行 `mimofan exec -c "/fleet --help"` THEN 系统 SHALL 返回帮助信息或执行结果
- WHEN 运行 `mimofan exec -c "/provider --help"` THEN 系统 SHALL 返回帮助信息或执行结果
- WHEN 运行 `mimofan exec -c "/stash --help"` THEN 系统 SHALL 返回帮助信息或执行结果
- WHEN 运行 `mimofan exec -c "/subagents --help"` THEN 系统 SHALL 返回帮助信息或执行结果
- WHEN 运行 `mimofan exec -c "/voice --help"` THEN 系统 SHALL 返回帮助信息或执行结果
- WHEN 运行 `mimofan exec -c "/yolo --help"` THEN 系统 SHALL 返回帮助信息或执行结果

**测试方法：** Shell 脚本 + `mimofan exec -c` 非交互模式

### 2. Fleet 管理验收测试（2 项）

**用户故事：** 作为开发者，我需要验证 Fleet 舰队系统的管理功能可被调用，以便确认子系统集成完整性。

**验收标准：**

- WHEN 运行 `mimofan fleet --help` THEN 系统 SHALL 返回舰队管理帮助信息
- WHEN 运行 `mimofan fleet list` THEN 系统 SHALL 返回舰队列表或空列表

**测试方法：** Shell 脚本 + CLI 命令

### 3. 按奥卡姆剃刀跳过项（1 项）

- `shell_output` — 内部辅助函数，非模型可见工具，无独立验收价值

### 4. 按运行时依赖跳过项（11 项）

以下项需要运行时环境，无法通过纯 API/CLI 测试验证：

| 类别 | 项目 | 原因 |
|------|------|------|
| MCP 集成 | 服务器连接、工具调用、超时、OAuth（4 项） | 需要 MCP 服务器进程 |
| Skills 系统 | 发现、安装、执行（3 项） | 需要 `~/.mimofan/skills/` 目录 |
| Runtime API | SSE 事件流、会话管理、工作区管理（3 项） | 需要 app-server 运行 |
| 命令系统 | /skills install（1 项） | 需要 Skills 运行时 |

## 设计约束

1. **MECE 原则**：每项测试互不重叠，所有可测试项完全穷尽
2. **奥卡姆剃刀**：只测试有独立验收价值的功能，跳过内部辅助函数
3. **无冗余代码**：复用现有测试框架模式（`cli_commands_test.sh`）
4. **无向下兼容**：直接使用最新 API，不保留旧接口

## 测试脚本规划

| 脚本 | 覆盖范围 | 预计测试项 |
|------|----------|------------|
| `benchmark/tui_commands_test.sh` | TUI 交互式命令 | 13 项 |
| `benchmark/fleet_test.sh` | Fleet 舰队管理 | 2 项 |

## 预期覆盖率

| 类别 | 当前 | 目标 | 变化 |
|------|------|------|------|
| 内置工具 | 41/42 | 41/42 | 不变（跳过内部辅助） |
| 命令系统 | 48/61 | 61/61 | +13 |
| 子系统集成 | 7/9 | 9/9 | +2 |
| **总计** | **111/127** | **126/127** | **+15** |
| **覆盖率** | **87%** | **99%** | **+12%** |

> 注：剩余 1%（shell_output）为内部辅助函数，按设计跳过。
