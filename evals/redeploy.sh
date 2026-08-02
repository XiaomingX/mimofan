#!/bin/bash
# mimofan 一站式构建脚本：fmt → clippy → test → build
# 用法: ./redeploy.sh [options]
#   --clean      清理构建缓存后再构建
#   --skip-fmt   跳过格式检查
#   --skip-clippy 跳过 clippy 检查
#   --skip-test  跳过测试
#   --debug      构建 debug 版本（默认 release）
#   -h, --help   显示帮助

set -e

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

CLEAN=false
SKIP_FMT=false
SKIP_CLIPPY=false
SKIP_TEST=false
RELEASE=true

while [[ $# -gt 0 ]]; do
    case $1 in
        --clean)       CLEAN=true; shift ;;
        --skip-fmt)    SKIP_FMT=true; shift ;;
        --skip-clippy) SKIP_CLIPPY=true; shift ;;
        --skip-test)   SKIP_TEST=true; shift ;;
        --debug)       RELEASE=false; shift ;;
        -h|--help)
            sed -n '2,/^$/p' "$0" | sed 's/^# \?//'
            exit 0 ;;
        *) echo -e "${RED}未知选项: $1${NC}"; exit 1 ;;
    esac
done

VERSION=$(sed -n '/^\[workspace\.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
DATE=$(date +%Y%m%d)
ARCH=$(uname -m)

echo -e "${BLUE}=== mimofan 构建 ===${NC}"
echo -e "${BLUE}版本: ${VERSION} | 日期: ${DATE} | 架构: ${ARCH}${NC}"
echo ""

# 1. 清理
if [ "$CLEAN" = true ]; then
    echo -e "${YELLOW}--- 清理构建缓存 ---${NC}"
    cargo clean
    echo ""
fi

# 2. 格式检查
if [ "$SKIP_FMT" = false ]; then
    echo -e "${YELLOW}--- 格式检查 ---${NC}"
    cargo fmt --all -- --check
    echo -e "${GREEN}✅ 格式检查通过${NC}"
    echo ""
fi

# 3. Clippy 检查
if [ "$SKIP_CLIPPY" = false ]; then
    echo -e "${YELLOW}--- Clippy 检查 ---${NC}"
    cargo clippy --workspace --all-features --locked -- \
        -D warnings \
        -A clippy::uninlined_format_args \
        -A clippy::too_many_arguments \
        -A clippy::unnecessary_map_or \
        -A clippy::assertions_on_constants
    echo -e "${GREEN}✅ Clippy 检查通过${NC}"
    echo ""
fi

# 4. 测试
if [ "$SKIP_TEST" = false ]; then
    echo -e "${YELLOW}--- 测试 ---${NC}"
    cargo test --workspace --locked
    echo -e "${GREEN}✅ 测试通过${NC}"
    echo ""
fi

# 5. 构建
echo -e "${YELLOW}--- 构建 ---${NC}"
if [ "$RELEASE" = true ]; then
    cargo build --release -p mimofan --locked
    BUILD_DIR="target/release"
    echo -e "${GREEN}✅ Release 构建完成${NC}"
else
    cargo build -p mimofan --locked
    BUILD_DIR="target/debug"
    echo -e "${GREEN}✅ Debug 构建完成${NC}"
fi
echo ""

# 6. 构建产物
echo -e "${YELLOW}--- 构建产物 ---${NC}"
ls -lh "$BUILD_DIR"/mimofan 2>/dev/null || true
echo ""
echo -e "${BLUE}=== 完成 ===${NC}"
