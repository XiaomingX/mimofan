#!/bin/bash
# 清理 TUI 中空的 #[cfg(test)] 模块声明
# 生成日期: 2026-08-04

set -e

TUI_SRC="/Users/a0000/mywork/commonLLM/opensource/nnnew/agent-mimofan/crates/tui/src"

echo "=== 清理空测试模块 ==="

# 查找空测试模块
count=0
for f in $(grep -rl "#\[cfg(test)\]" "$TUI_SRC" 2>/dev/null); do
    # 检查是否有实际测试
    test_count=$(grep -c "^#\[test\]" "$f" 2>/dev/null)
    if [ "$test_count" -eq 0 ]; then
        # 检查是否是空模块声明
        if grep -q "^#\[cfg(test)\]$" "$f" 2>/dev/null; then
            rel_path=$(echo $f | sed "s|$TUI_SRC/||")
            echo "  清理: $rel_path"

            # 使用 sed 删除空的 #[cfg(test)] 模块
            # 匹配模式: #[cfg(test)] 后跟空行和 mod xxx {}
            sed -i '' '/^#\[cfg(test)\]$/{
                N
                /^#\[cfg(test)\]\nmod [a-z_]* {};$/d
            }' "$f"

            count=$((count + 1))
        fi
    fi
done

echo -e "\n=== 完成 ==="
echo "已清理 $count 个空测试模块"
