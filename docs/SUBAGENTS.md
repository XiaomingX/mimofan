# 子 Agent

子 agent 是 mimofan 的嵌套任务执行机制。父 agent 通过 `agent` 工具启动子 agent，子 agent 完成后返回结果。

## 角色类型

`type` 字段选择子 agent 的角色（系统提示词）：

| 角色 | 说明 | 可写文件 | 可执行 shell |
|------|------|:--------:|:-----------:|
| `general` | 通用，默认角色 | ✅ | ✅ |
| `explore` | 只读探索，快速定位代码 | ❌ | 只读 |
| `plan` | 分析规划，不执行 | 最少 | 最少 |
| `review` | 代码审查，给出评分 | ❌ | 只读 |
| `implementer` | 实现具体改动 | ✅ | ✅ |
| `verifier` | 运行测试验证 | ❌ | 测试专用 |
| `custom` | 自定义工具白名单 | 视配置 | 视配置 |

## 使用方式

```
agent(
  task: "找出所有调用 Foo.bar 的地方",
  type: "explore"
)
```

## 上下文继承

- 默认：子 agent 从空白开始，只带角色提示 + 任务描述
- `fork_context: true`：继承父 agent 的当前上下文（适合续写、审查、总结）

## 并发与深度

- 最大并发数：`max_subagents`（默认 20，范围 1-20）
- 子 agent 不能再嵌套 `agent` 工具（叶子节点）
- 取消父 turn 不会杀死已启动的子 agent

## 配置

```toml
[subagents]
max_concurrent = 20
max_depth = 6

# 按服务商限制
[subagents.providers.deepseek]
max_concurrent = 20
```
