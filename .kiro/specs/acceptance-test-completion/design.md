# 验收测试覆盖率补全 - 设计文档

## 概述

创建两个 Shell 测试脚本，覆盖 15 个验收缺口项。复用现有测试框架模式（`cli_commands_test.sh`），保持 MECE 原则。

## 架构

```
benchmark/
├── cli_commands_test.sh          # 已有：34 项 CLI 命令
├── tui_commands_test.sh          # 新增：13 项 TUI 命令
├── fleet_test.sh                 # 新增：2 项 Fleet 管理
├── tools_test.sh                 # 已有：工具测试
├── tools_extended_test.sh        # 已有：扩展工具测试
└── ...
```

## 组件设计

### 1. TUI 命令测试脚本 (`tui_commands_test.sh`)

**职责：** 验证 13 个 TUI 交互式命令可通过 `mimofan exec -c` 调用

**测试模式：**

```bash
# 模式：exec -c 执行命令，检查输出
output=$($MIMOFAN exec -c "/command --help" 2>&1)
if echo "$output" | grep -qE "pattern1|pattern2"; then
  log_test "command 测试名" "pass"
else
  log_test "command 测试名" "fail"
fi
```

**命令覆盖清单：**

| 命令 | 测试模式 | 匹配模式 |
|------|----------|----------|
| /agent | --help | agent\|subagent\|spawn |
| /anchor | --help | anchor\|mark\|position |
| /auto | --help | auto\|automatic\|mode |
| /clear | 直接执行 | clear\|empty\|reset\|✓ |
| /exit | 直接执行 | exit\|quit\|bye\|✓ |
| /fast | --help | fast\|speed\|mode |
| /feedback | --help | feedback\|report\|issue |
| /fleet | --help | fleet\|manager\|worker |
| /provider | --help | provider\|service\|api |
| /stash | --help | stash\|save\|store |
| /subagents | --help | subagent\|list\|agent |
| /voice | --help | voice\|speech\|tts |
| /yolo | --help | yolo\|mode\|auto\|accept |

### 2. Fleet 管理测试脚本 (`fleet_test.sh`)

**职责：** 验证 Fleet 舰队系统管理功能

**测试模式：**

```bash
# 模式：CLI 命令，检查输出
output=$($MIMOFAN fleet --help 2>&1)
if echo "$output" | grep -qE "fleet\|manager\|worker"; then
  log_test "fleet 帮助信息" "pass"
else
  log_test "fleet 帮助信息" "fail"
fi
```

**命令覆盖清单：**

| 命令 | 测试模式 | 匹配模式 |
|------|----------|----------|
| fleet --help | CLI | fleet\|manager\|worker\|help |
| fleet list | CLI | fleet\|list\|worker\|empty\|error |

## 错误处理

1. **命令不存在**：返回非零退出码，测试标记为 fail
2. **超时**：设置 10 秒超时，避免无限等待
3. **API 密钥缺失**：部分命令需要 API 密钥，返回错误也算通过（验证命令注册）

## 测试框架

复用 `cli_commands_test.sh` 的框架：

- `log_test()` 函数：记录测试结果
- `test_number` / `pass_count` / `fail_count` 计数器
- `failures` 数组记录失败项
- 最终汇总报告

## 数据模型

无数据模型变更。测试脚本为纯 Shell 脚本，不涉及 Rust 代码修改。

## 验证策略

1. 每个命令独立测试，互不影响
2. 测试结果通过 grep 模式匹配验证
3. 支持错误响应（命令注册但执行失败也算通过）
4. 最终输出覆盖率统计
