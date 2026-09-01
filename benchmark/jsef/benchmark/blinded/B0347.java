/*
 * JSEF Benchmark 样本 — 存储型 XSS 安全对照（CWE-79，L2）
 * 取出后做 HTML 编码 / ESAPI 转义再渲染。
 * 安全底线：仅 localhost 演示语义。
 */
package blinded;

public class XssStoredBy {

    static String db = "";

    
    static String storeAndRender(String userInput) {
        db = userInput;
        String fromDb = db;
        String by = htmlEncode(fromDb);     // HTML 编码
        // source：不可信用户输入（已落库），但经 HTML 编码
        /*ANCHOR_1*/
        return "<html><body>" + by + "</body></html>";
    }

    // 模拟 ESAPI.encoder().encodeForHTML
    static String htmlEncode(String s) {
        return s.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")
                .replace("\"", "&quot;").replace("'", "&#x27;");
    }
}
