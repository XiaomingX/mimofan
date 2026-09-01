/*
 * JSEF Benchmark 样本 — DOM 型 XSS 安全对照（CWE-79，L2）
 * 使用 textContent 或转义后再写入 DOM。
 * 安全底线：仅 localhost 演示语义。
 */
package blinded;

public class XssDomBy {

    
    static String renderPage(String userInput) {
        String by = jsEscape(userInput);    // JS 字符串转义
        // source：不可信用户输入，但已转义后作为纯文本写入
        /*ANCHOR_1*/
        return "<html><body><script>var x = \"" + by + "\"; document.getElementById('out').textContent = x;</script></body></html>";
    }

    static String jsEscape(String s) {
        return s.replace("\\", "\\\\").replace("\"", "\\\"").replace("<", "\\u003c");
    }
}
