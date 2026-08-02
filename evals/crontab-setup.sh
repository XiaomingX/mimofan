#!/bin/bash
# 设置 cargo 清理脚本的 crontab 定时任务
# 用法: ./crontab-setup.sh

set -e

# 颜色定义
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo -e "${GREEN}=== 设置 cargo 清理脚本 crontab 定时任务 ===${NC}"
echo ""

# 1. 备份当前 crontab
echo -e "${YELLOW}1. 备份当前 crontab...${NC}"
BACKUP_FILE="/tmp/crontab_backup_$(date +%Y%m%d_%H%M%S).txt"
crontab -l > "$BACKUP_FILE" 2>/dev/null || echo "当前没有 crontab 配置"
echo -e "${GREEN}备份完成: $BACKUP_FILE${NC}"
echo ""

# 2. 检查是否已存在清理任务
echo -e "${YELLOW}2. 检查是否已存在清理任务...${NC}"
if crontab -l 2>/dev/null | grep -q ".cargo-clean.sh"; then
    echo -e "${YELLOW}警告: 已存在清理任务，将更新配置${NC}"
    # 删除旧的清理任务
    crontab -l 2>/dev/null | grep -v ".cargo-clean.sh" | crontab -
fi

# 3. 添加新的清理任务
echo -e "${YELLOW}3. 添加每日 10:20 运行清理脚本的定时任务...${NC}"
(crontab -l 2>/dev/null; echo "20 10 * * * /Users/a0000/mywork/commonLLM/opensource/nnnew/agent-mimofan/.cargo-clean.sh --keep-days 1 >> /Users/a0000/mywork/commonLLM/opensource/nnnew/agent-mimofan/cron.log 2>&1") | crontab -

# 4. 验证任务是否添加成功
echo -e "${YELLOW}4. 验证任务是否添加成功...${NC}"
if crontab -l 2>/dev/null | grep -q ".cargo-clean.sh"; then
    echo -e "${GREEN}任务添加成功！${NC}"
    echo ""
    echo "当前 crontab 配置:"
    crontab -l 2>/dev/null | grep ".cargo-clean.sh"
else
    echo -e "${RED}任务添加失败！${NC}"
    exit 1
fi
echo ""

# 5. 显示任务详情
echo -e "${YELLOW}5. 任务详情...${NC}"
echo "执行时间: 每日 10:20"
echo "执行命令: .cargo-clean.sh --keep-days 1"
echo "日志文件: /Users/a0000/mywork/commonLLM/opensource/nnnew/agent-mimofan/cron.log"
echo "备份文件: $BACKUP_FILE"
echo ""

# 6. 测试脚本
echo -e "${YELLOW}6. 测试清理脚本...${NC}"
cd /Users/a0000/mywork/commonLLM/opensource/nnnew/agent-mimofan
if ./.cargo-clean.sh --dry-run 2>&1 | grep -q "预览模式"; then
    echo -e "${GREEN}脚本测试通过！${NC}"
else
    echo -e "${YELLOW}脚本测试失败，请检查！${NC}"
fi
echo ""

echo -e "${GREEN}=== 设置完成 ===${NC}"
echo ""
echo "提示："
echo "  - 查看 crontab: crontab -l"
echo "  - 编辑 crontab: crontab -e"
echo "  - 删除清理任务: crontab -l | grep -v '.cargo-clean.sh' | crontab -"
echo "  - 查看日志: tail -f /Users/a0000/mywork/commonLLM/opensource/nnnew/agent-mimofan/cron.log"
