// [VULN]
package com.jsef.benchmark.vuln;

/**
 * JSEF-Benchmark — 深层嵌套 JSON/XML 递归解析拒绝服务 (CWE-400，难度 L2)
 *
 * 危险入口：手写递归下降解析器未限制嵌套深度，输入为深度 >10000 的嵌套数组/标签时，
 * 每层 '[' 都叠加一次调用栈，导致 StackOverflowError / CPU DoS。
 *
 * 说明：此处语义是 CWE-400 的资源耗尽（深度无上限），区别于实体展开的 CWE-776（XmlBombVuln.java）。
 *
 * 安全底线：仅 localhost 演示语义，不提供真实 DoS 载荷。
 * 修复要点（DeepNestingDosSafe.java）：深度计数器 + MAX_DEPTH 上限拒绝。
 */
public class DeepNestingDosVuln {

    /**
     * 危险：递归下降解析，无 maxDepth 上限，超深输入即栈溢出。
     */
    static int parseDepth(String s, int i) {
        if (i >= s.length() || s.charAt(i) != '[') {
            return i; // 叶节点 / 非 '[' 结束
        }
        // [CHECKPOINT id=JSEF-DEPTH-001 cwe=400 level=L2 source=deeply nested json/xml sink=recursive parse without depth limit → stack overflow expect=VULN]
        int j = parseDepth(s, i + 1); // 每层 '[' 无限递归，深度无上限 → 栈溢出
        return s.charAt(j) == ']' ? j + 1 : j;
    }

    public int parse(String input) {
        return parseDepth(input, 0);
    }

    public static void main(String[] args) {
        // 演示语义：仅说明嵌套深度 > 栈深时触发 StackOverflowError，不提供真实载荷
        new DeepNestingDosVuln().parse("[".repeat(20000) + "]".repeat(20000));
    }
}
