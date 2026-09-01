/*
 * JSEF Benchmark 样本 — DOM 型 XSS（CWE-79，L2）
 * 服务端把用户输入回显到页面 JS 上下文，客户端脚本执行。
 * 安全底线：仅 localhost 演示语义，不写真实攻击利用脚本。
 */
package blinded;

public class XssDom {

    
    static String renderPage(String userInput) {
        // source：不可信用户输入（HTTP 请求参数）
        // 回显到 JS 变量后由客户端写入 DOM
        /*ANCHOR_1*/
        return "<html><body><script>var x = \"" + userInput + "\"; document.getElementById('out').innerHTML = x;</script></body></html>";
    }
}
