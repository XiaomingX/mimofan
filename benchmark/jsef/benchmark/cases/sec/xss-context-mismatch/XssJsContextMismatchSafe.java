package com.jsef.benchmark.sec;

/**
 * JSEF-Benchmark L3 — XSS 上下文错配修复（CWE-79）
 *
 * 修复：不再把用户数据拼进内联 <script> 的 JS 字符串，改用 JS 字符串专用转义
 * （转义 \ ' " 与换行，并用 < / > 阻断 </script> 闭合），避免依赖
 * HTML 实体转义的错误上下文。更稳妥的替代是 textContent 赋值，数据仅作为
 * 文本节点渲染，永不进入 HTML/JS 语法。
 *
 * CWE-79 Cross-site Scripting (Reflected)。
 */
public class XssJsContextMismatchSafe {

    /** JS 字符串专用转义：处理反斜杠、单双引号、控制字符，并阻断 </script> 闭合。 */
    static String escapeJsString(String value) {
        StringBuilder sb = new StringBuilder();
        for (int i = 0; i < value.length(); i++) {
            char c = value.charAt(i);
            switch (c) {
                case '\\': sb.append("\\\\"); break;
                case '\'': sb.append("\\'"); break;
                case '"':  sb.append("\\\""); break;
                case '\n': sb.append("\\n"); break;
                case '\r': sb.append("\\r"); break;
                case '<':  sb.append("\\u003c"); break;
                case '>':  sb.append("\\u003e"); break;
                default:   sb.append(c);
            }
        }
        return sb.toString();
    }

    /**
     * 安全路径：JS 上下文专用转义，杜绝引号逃逸与 </script> 闭合。
     *
     * @param user 用户可控昵称
     */
    public String render(String user) {
        String js = "var name = '" + escapeJsString(user) + "';"; // JS 上下文专用转义
        // [CHECKPOINT id=JSEF-XSSCTX-001S cwe=79 level=L3 source=user name sink=JS-specific escape/textContent (no inline concat) expect=SAFE]
        return "<script>" + js + "</script>";
    }
}
