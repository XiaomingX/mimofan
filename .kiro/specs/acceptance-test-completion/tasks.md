# 实现计划

- [x] 1. 创建 TUI 命令测试脚本
  - 创建 `benchmark/tui_commands_test.sh`
  - 复用 `cli_commands_test.sh` 的框架结构
  - 实现 13 个 TUI 命令的验收测试
  - _Requirements: 1_

- [x] 1.1 实现测试框架
  - 颜色定义、计数器、log_test 函数
  - MIMOFAN 二进制路径配置
  - 测试结果汇总输出
  - _Requirements: 1_

- [x] 1.2 实现 Core 命令测试（7 项）
  - /agent --help
  - /anchor --help
  - /auto --help
  - /clear 直接执行
  - /exit 直接执行
  - /fast --help
  - /feedback --help
  - _Requirements: 1_

- [x] 1.3 实现其他命令测试（6 项）
  - /fleet --help
  - /provider --help
  - /stash --help
  - /subagents --help
  - /voice --help
  - /yolo --help
  - _Requirements: 1_

- [x] 2. 创建 Fleet 管理测试脚本
  - 创建 `benchmark/fleet_test.sh`
  - 实现 2 个 Fleet 命令的验收测试
  - _Requirements: 2_

- [x] 2.1 实现 fleet --help 测试
  - 验证舰队管理帮助信息输出
  - _Requirements: 2_

- [x] 2.2 实现 fleet list 测试
  - 验证舰队列表命令可执行
  - _Requirements: 2_

- [x] 3. 运行测试验证
  - 执行 `benchmark/tui_commands_test.sh` 验证全部通过
  - 执行 `benchmark/fleet_test.sh` 验证全部通过
  - 更新 `benchmark_to_test.md` 覆盖率统计
  - _Requirements: 1, 2_
