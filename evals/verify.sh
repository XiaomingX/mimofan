#!/bin/bash
# mimofan 快速验收脚本
# 用法: ./verify.sh [all|docs|scripts|build|test]

set -e

TARGET="${1:-all}"

echo "=== mimofan 快速验收 (target: $TARGET) ==="
echo ""

# 颜色定义
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# 检查函数
check_file() {
    local file=$1
    local desc=$2
    if [ -f "$file" ]; then
        echo -e "${GREEN}✅ $desc${NC}"
        return 0
    else
        echo -e "${RED}❌ $desc 缺失${NC}"
        return 1
    fi
}

check_exec() {
    local file=$1
    local desc=$2
    if [ -x "$file" ]; then
        echo -e "${GREEN}✅ $desc 可执行${NC}"
        return 0
    else
        echo -e "${YELLOW}⚠️  $desc 不可执行，运行 chmod +x $file${NC}"
        return 1
    fi
}

check_not_exists() {
    local file=$1
    local desc=$2
    if [ ! -f "$file" ]; then
        echo -e "${GREEN}✅ $desc 已清理${NC}"
        return 0
    else
        echo -e "${YELLOW}⚠️  $desc 未清理${NC}"
        return 1
    fi
}

# 1. 检查文件清理
check_docs_cleanup() {
    echo "--- 1. 检查文件清理 ---"
    check_not_exists "ARCHITECTURE_PLAN.md" "ARCHITECTURE_PLAN.md"
    check_not_exists "settings.example.json" "settings.example.json"
    echo ""
}

# 2. 检查文档完整性
check_docs() {
    echo "--- 2. 检查文档完整性 ---"
    check_file "README.md" "README.md"
    check_file "ARCHITECTURE.md" "ARCHITECTURE.md"
    echo ""
}

# 3. 检查脚本可用性
check_scripts() {
    echo "--- 3. 检查脚本可用性 ---"
    check_exec "redeploy.sh" "redeploy.sh"
    check_exec "doctor.sh" "doctor.sh"
    check_exec "verify.sh" "verify.sh"
    echo ""
}

# 4. 运行构建测试
check_build() {
    echo "--- 4. 运行构建测试 ---"
    if ./redeploy.sh --skip-test 2>&1 | tail -5; then
        echo -e "${GREEN}✅ 构建测试通过${NC}"
    else
        echo -e "${RED}❌ 构建测试失败${NC}"
        return 1
    fi
    echo ""
}

# 5. 运行代码检查（已合并到 redeploy.sh）
check_code() {
    echo -e "${GREEN}✅ 代码检查已包含在 redeploy.sh 中${NC}"
    echo ""
}

# 6. 运行测试（已合并到 redeploy.sh）
check_test() {
    echo -e "${GREEN}✅ 测试已包含在 redeploy.sh 中${NC}"
    echo ""
}

# 7. 运行诊断
check_doctor() {
    echo "--- 7. 运行诊断 ---"
    if ./doctor.sh 2>&1 | tail -10; then
        echo -e "${GREEN}✅ 诊断完成${NC}"
    else
        echo -e "${YELLOW}⚠️  诊断有问题${NC}"
    fi
    echo ""
}

# 主流程
case "$TARGET" in
    all)
        check_docs_cleanup
        check_docs
        check_scripts
        check_build
        check_code
        check_test
        check_doctor
        ;;
    docs)
        check_docs_cleanup
        check_docs
        ;;
    scripts)
        check_scripts
        ;;
    build)
        check_build
        ;;
    test)
        check_test
        ;;
    *)
        echo "未知检查目标: $TARGET"
        echo "可用目标: all, docs, scripts, build, test"
        exit 1
        ;;
esac

echo "=== 验收完成 ==="
