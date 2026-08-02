# 经验教训总结

> 本次验收过程中遇到的问题和解决方案，供后续类似任务参考。

**最后更新**: 2026-08-02

---

## 1. 跨平台兼容性问题

### 1.1 timeout 命令不存在
**问题**: macOS 没有 `timeout` 命令，导致脚本失败。

**解决方案**:
```bash
# 跨平台 timeout 函数
run_with_timeout() {
    local timeout_seconds=$1
    shift
    if command -v timeout &> /dev/null; then
        timeout "$timeout_seconds" "$@"
    else
        "$@" &
        local pid=$!
        (sleep "$timeout_seconds" && kill -9 $pid 2>/dev/null) &
        local watchdog=$!
        wait $pid 2>/dev/null
        local exit_code=$?
        kill $watchdog 2>/dev/null
        return $exit_code
    fi
}
```

**适用场景**: 所有需要超时控制的脚本

---

## 2. 错误处理问题

### 2.1 ls 命令无匹配时失败
**问题**: `ls -d target/debug/incremental/mimofan-*` 在没有匹配文件时返回错误，导致 `set -e` 退出脚本。

**解决方案**:
```bash
# ❌ 错误
ls -d target/debug/incremental/mimofan-*

# ✅ 正确
ls -d target/debug/incremental/mimofan-* 2>/dev/null || true
```

**适用场景**: 所有使用 `ls`、`find`、`grep` 等命令的脚本

### 2.2 find 命令目录不存在时失败
**问题**: `find` 命令在目标目录不存在时返回错误。

**解决方案**:
```bash
# ❌ 错误
find target/debug/incremental -maxdepth 1 -type d -exec rm -rf {} \;

# ✅ 正确
if [ -d "target/debug/incremental" ]; then
    find target/debug/incremental -maxdepth 1 -type d -exec rm -rf {} \; 2>/dev/null || true
fi
```

**适用场景**: 所有文件系统操作

---

## 3. 路径处理问题

### 3.1 source 时 $0 不是预期值
**问题**: 在 `source config.env` 时，`$0` 是调用脚本的路径，不是 config.env 的路径。

**解决方案**:
```bash
# ❌ 错误（在 config.env 中）
MIMOFAN_BIN="$(dirname "$0")/../target/release/mimofan"

# ✅ 正确（在 config.env 中）
MIMOFAN_BIN="${SCRIPT_DIR}/../target/release/mimofan"

# 在调用脚本中设置 SCRIPT_DIR
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/config.env"
```

**适用场景**: 所有使用 source 共享配置的脚本

---

## 4. API 错误处理

### 4.1 账户余额不足
**问题**: API 返回 HTTP 402 Payment Required insufficient_balance 错误。

**解决方案**:
- 这是外部服务问题，不是代码问题
- 脚本应该优雅地处理此类错误
- 在测试报告中明确标注为"外部服务问题"

**适用场景**: 所有调用外部 API 的测试脚本

---

## 5. 测试脚本设计

### 5.1 测试用例选择
**经验**:
- 优先测试核心功能（API 连接、基本对话）
- 次要测试高级功能（多 Agent、记忆压缩）
- 外部依赖测试应该可以跳过

**模板**:
```bash
# 核心测试（必须通过）
test_core() {
    # API 连接
    # 基本对话
    # 错误处理
}

# 高级测试（可选）
test_advanced() {
    # 多 Agent
    # 记忆压缩
    # /rewind
}
```

### 5.2 测试结果报告
**经验**:
- 使用 JSON 格式记录结果
- 生成人类可读的摘要
- 区分"代码问题"和"外部服务问题"

---

## 6. 脚本组织

### 6.1 配置文件
**经验**:
- 使用统一的 `config.env` 管理配置
- 支持环境变量覆盖
- 提供合理的默认值

**模板**:
```bash
# config.env
API_KEY="${ANTHROPIC_API_KEY:-default_key}"
MODEL="${MODEL:-mimo-v2.5}"
TIMEOUT_SECONDS="${TIMEOUT:-30}"
```

### 6.2 脚本分层
**经验**:
- `release_check.sh` - 发布前验收（必须通过）
- `test_api_endpoints.sh` - API 端点测试
- `test_mimofan_integration.sh` - 集成测试
- `tui_benchmark.sh` - 性能基准测试

---

## 7. 调试技巧

### 7.1 使用 bash -x 调试
```bash
bash -x script.sh 2>&1 | tee debug.log
```

### 7.2 检查中间状态
```bash
# 检查变量
echo "DEBUG: MIMOFAN_BIN=$MIMOFAN_BIN"

# 检查命令是否存在
command -v timeout &> /dev/null && echo "timeout exists" || echo "timeout missing"
```

### 7.3 使用 set -x 和 set +x
```bash
set -x  # 开启调试
# 调试代码
set +x  # 关闭调试
```

---

## 快速参考卡

### 跨平台脚本检查清单
- [ ] 是否使用了 `timeout` 命令？→ 使用 `run_with_timeout` 函数
- [ ] 是否使用了 `ls`、`find`、`grep`？→ 添加 `|| true`
- [ ] 是否使用了 `source`？→ 使用 `BASH_SOURCE[0]` 而非 `$0`
- [ ] 是否删除文件/目录？→ 先检查是否存在

### 错误处理检查清单
- [ ] 是否使用了 `set -e`？→ 所有命令添加 `|| true`
- [ ] 是否调用外部 API？→ 处理 4xx/5xx 错误
- [ ] 是否操作文件系统？→ 检查目录/文件是否存在
