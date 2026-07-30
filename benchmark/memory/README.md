# Memory System Benchmarks

针对 mimofan 记忆系统的性能和功能测试。

## 测试类型

### 1. 功能测试 (memory_functional_test)
验证所有记忆系统功能正常工作：
- 向量存储：存储、搜索、过滤
- 批量处理器：入队、处理
- 速率限制器：检查
- 任务管理器：启动、完成
- 压缩器：分析、总结
- 搜索缓存：插入、获取

### 2. 集成测试 (memory_integration_test)
测试完整工作流程：
- 存储 observations
- 搜索和过滤
- 压缩 observations

### 3. 性能基准测试 (memory_benchmark)
测量各模块的性能指标：
- 向量存储操作
- 批量处理
- 速率限制
- 任务管理
- 压缩操作
- 缓存操作

## 运行测试

### 运行所有测试
```bash
./run_all_tests.sh
```

### 单独运行功能测试
```bash
cargo run --release -p mimofan-memory --example memory_functional_test
```

### 单独运行集成测试
```bash
cargo run --release -p mimofan-memory --example memory_integration_test
```

### 单独运行性能基准测试
```bash
cargo run --release -p mimofan-memory --example memory_benchmark
```

## 性能基准结果

### 向量存储
- 存储 observations: ~1000 ops/s
- 搜索 (k=10): ~5000 ops/s
- 带过滤搜索: ~4000 ops/s

### 批量处理
- 入队 10k observations: ~2.8M ops/s
- 处理所有批次: ~9.1M ops/s

### 速率限制
- 检查速率限制: ~16.9M ops/s

### 任务管理
- 启动 1000 个任务: ~3.1M ops/s
- 完成 1000 个任务: ~828K ops/s

### 压缩
- 分析 1000 observations: ~70K ops/s
- 总结会话: ~68K ops/s

### 搜索缓存
- 插入 1000 条目: ~12.5M ops/s
- 获取 1000 条目: ~8.6M ops/s

## 文件结构

```
benchmark/memory/
├── README.md                    # 本文档
├── memory_benchmark.rs          # 性能基准测试
├── memory_functional_test.rs    # 功能测试
├── memory_integration_test.rs   # 集成测试
├── run_all_tests.sh             # 运行所有测试脚本
└── run_benchmark.sh             # 运行基准测试脚本
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
