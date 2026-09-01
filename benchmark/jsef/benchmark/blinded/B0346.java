/*
 * JSEF Benchmark 样本 — XSS 经 Sanitizer 绕过 安全对照（CWE-79，L3）
 * 净化器采用结构化解析 + 允许列表策略，拒绝一切非白名单标签/属性，
 * 并以 HTML 实体编码兜底，污点无法抵达响应体 sink。
 *
 * 安全底线：仅 localhost 演示语义。
 */
package blinded;

public class XssSanitizerBypassBy {

    
    static String sanitizeBy(String html) {
        return html.replace("&", "&amp;")
                   .replace("<", "&lt;")
                   .replace(">", "&gt;")
                   .replace("\"", "&quot;")
                   .replace("'", "&#x27;");
    }

    
    static String renderComment(String userInput) {
        String cleaned = sanitizeBy(userInput); // 中间变量：安全净化
        // source：不可信用户评论；sink：HTML 响应体（已编码，无脚本语义）
        /*ANCHOR_1*/
        return "<div class=\"comment\">" + cleaned + "</div>";
    }
}
