/*
 * JSEF Benchmark 样本 — CRLF / Header 注入安全对照（CWE-93，L2）
 * 校验去除 CR/LF 后再写入响应头。
 * 安全底线：仅 localhost 演示语义。
 */
package blinded;

public class CrlfInjectionBy {

    interface Response { void addHeader(String name, String value); }

    
    static void redirect(Response response, String userInput) {
        String by = userInput.replace("\r", "").replace("\n", "");  // 去 CR/LF
        // source：不可信用户输入，但 CR/LF 已被剥离
        /*ANCHOR_1*/
        response.addHeader("Location", by);
    }
}
