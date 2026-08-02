# Memory System Benchmarks

针对 mimofan 记忆系统的性能和功能测试。

## 测试类型

测试代码已迁移到 `crates/memory/examples/` 目录：

1. **功能测试** (`memory_functional_test.rs`) - 验证所有记忆系统功能正常工作
2. **集成测试** (`memory_integration_test.rs`) - 测试完整工作流程
3. **性能基准测试** (`memory_benchmark.rs`) - 测量各模块的性能指标

## 运行测试

```bash
# 运行功能测试
cargo run --release -p mimofan-memory --example memory_functional_test

# 运行集成测试
cargo run --release -p mimofan-memory --example memory_integration_test

# 运行性能基准测试
cargo run --release -p mimofan-memory --example memory_benchmark

# 运行性能基准测试（另一个版本）
cargo run --release -p mimofan-memory --example performance_benchmark
```

## 测试覆盖

### 功能覆盖
✅ 向量存储 CRUD 操作
✅ 相似性搜索
✅ 搜索过滤
✅ 批量处理
✅ 速率限制
✅ 任务管理
✅ 压缩分析
✅ 会话总结
✅ 搜索缓存

### 性能覆盖
✅ 存储性能
✅ 搜索性能
✅ 批处理性能
✅ 速率限制性能
✅ 任务管理性能
✅ 压缩性能
✅ 缓存性能

## 注意事项

1. **依赖项**: 需要先编译 release 版本
2. **临时文件**: 测试使用临时目录，会自动清理
3. **性能指标**: 结果可能因硬件而异
4. **运行时间**: 完整测试约需 2-3 分钟

## 集成到 CI

可以将这些测试添加到 CI 流程中：

```yaml
# .github/workflows/memory-tests.yml
name: Memory System Tests

on: [push, pull_request]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v2
      - name: Run memory tests
        run: |
          cargo test -p mimofan-memory
          cargo run --release -p mimofan-memory --example memory_functional_test
          cargo run --release -p mimofan-memory --example memory_integration_test
```
