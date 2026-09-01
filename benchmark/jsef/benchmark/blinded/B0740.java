/*
 * JSEF Benchmark 样本 — 存储型 XSS（CWE-79，L2）
 * 用户输入存入数据库（模拟），取出后未转义直接渲染到 HTML 响应。
 * 安全底线：仅 localhost 演示语义，不写真实攻击利用脚本。
 */
package blinded;

public class XssStored {

    // 模拟数据库存储
    static String db = "";

    
    static String storeAndRender(String userInput) {
        db = userInput;                       // 存入"数据库"
        String fromDb = db;                   // 从"数据库"取出
        // source：不可信用户输入（HTTP 请求，已落库）
        /*ANCHOR_1*/
        return "<html><body>" + fromDb + "</body></html>";  // 未转义直接拼接 → 存储型 XSS
    }
}
