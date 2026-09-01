package com.jsef.benchmark.sec;

/**
 * JSEF-Benchmark — 深层嵌套 JSON/XML 解析拒绝服务修复 (CWE-400，难度 L2)
 *
 * 修复：解析前维护深度计数器 depth，每层递归 +1，超过 MAX_DEPTH=128 立即抛异常，拒绝超深输入。
 */
public class DeepNestingDosSafe {

    static final int MAX_DEPTH = 128;

    /**
     * 安全：depth 递增计数，超限抛异常，杜绝无上限递归导致的栈溢出。
     */
    static int parseDepth(String s, int i, int depth) throws Exception {
        if (depth > MAX_DEPTH) {
            throw new Exception("nesting depth exceeds limit " + MAX_DEPTH);
        }
        if (i >= s.length() || s.charAt(i) != '[') {
            return i; // 叶节点 / 非 '[' 结束
        }
        // [CHECKPOINT id=JSEF-DEPTH-001S cwe=400 level=L2 source=deeply nested json/xml sink=recursive parse without depth limit → stack overflow expect=SAFE]
        int j = parseDepth(s, i + 1, depth + 1); // 深度计数器 +1，超限即拒绝
        return s.charAt(j) == ']' ? j + 1 : j;
    }

    public int parse(String input) throws Exception {
        return parseDepth(input, 0, 0);
    }

    public static void main(String[] args) throws Exception {
        new DeepNestingDosSafe().parse("[1,2,3]");
    }
}
