package com.jsef.benchmark.vuln;

import org.springframework.web.util.HtmlUtils;

/**
 * JSEF-Benchmark L3 — XSS 上下文错配（CWE-79）
 *
 * 难度：L3（间接 / 跨方法）。污点 user name 经 HtmlUtils.htmlEscape（HTML 文本节点
 * 上下文的转义）后被拼入 <script> 内联单引号字符串 —— 转义上下文与使用上下文错配。
 *
 * 常见误判点："见过 escapeHtml 即判安全"。HTML 实体转义对 JS 字符串上下文无效：
 *   ① HtmlUtils.htmlEscape / escapeHtml4 不处理反斜杠与 <script> 闭合；
 *   ② <script> 内的字符不被 HTML 实体解码，字符串边界 ' 仍是 JS 语法字符；
 *   ③ 未闭合的 </script> 会提前终止 script 元素，使后续标记进入 HTML 解析器。
 *
 * 载荷示例（localhost 演示语义）：
 *   user = "'; alert(1); var x = '"
 *   user = "'; \n</script><img src=x onerror=alert(1)>"
 *
 * CWE-79 Cross-site Scripting (Reflected)。
 * 安全底线：Payload 仅 localhost 演示语义，不提供真实利用脚本。
 *
 * 修复要点（对照 XssJsContextMismatchSafe.java）：JS 专用转义 / textContent 赋值 / 不拼接进内联 <script>。
 */
public class XssJsContextMismatchVuln {

    /**
     * 危险路径：HTML 转义后的数据拼入 <script> 单引号字符串。
     *
     * @param user 用户可控昵称（source）
     */
    public String render(String user) {
        String escaped = HtmlUtils.htmlEscape(user); // 节点 1：HTML 上下文转义（对 JS 字符串无效）
        String js = "var name = '" + escaped + "';"; // 节点 2：拼入 <script> 单引号字符串
        // [CHECKPOINT id=JSEF-XSSCTX-001 cwe=79 level=L3 source=user name sink=script single-quote context after HtmlUtils.htmlEscape expect=VULN trace=benchmark/cases/vuln/xss-context-mismatch/XssJsContextMismatchVuln.java:33,benchmark/cases/vuln/xss-context-mismatch/XssJsContextMismatchVuln.java:34,benchmark/cases/vuln/xss-context-mismatch/XssJsContextMismatchVuln.java:36]
        return "<script>" + js + "</script>"; // [VULN] 节点 3（sink）：浏览器执行内联 <script>
    }
}
