/*
 * JSEF Benchmark 样本 — CRLF / Header 注入（CWE-93，L2）
 * 把含 \r\n 的用户输入写入响应头，导致响应头拆分 / 响应拆分。
 * 安全底线：仅 localhost 演示语义，不写真实攻击利用脚本。
 */
package blinded;

public class CrlfInjection {

    // 演示用响应头接口（语义同 HttpServletResponse）
    interface Response { void addHeader(String name, String value); }

    
    static void redirect(Response response, String userInput) {
        // source：不可信用户输入（HTTP 请求参数）
        /*ANCHOR_1*/
        response.addHeader("Location", userInput);   // 含 \r\n 的输入 → 头拆分
    }
}
