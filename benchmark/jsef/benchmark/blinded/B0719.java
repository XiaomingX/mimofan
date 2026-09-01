/*
 * JSEF Benchmark 样本 — 信任边界违规 (CWE-501, L3)
 * 不可信输入存入 HttpSession 当作可信数据，后续跨方法使用未校验。
 * 安全底线：仅 localhost 演示语义。
 */
package blinded;

public class TrustBoundarySession {

    // 模拟 Session
    interface Session { void setAttribute(String k, Object v); Object getAttribute(String k); }

    // source：不可信请求参数
    static void store(Session session, String userInput) {
        /*ANCHOR_1*/
        session.setAttribute("trustedData", userInput); // 不可信数据存入会话当可信
    }

    // 跨方法：从会话读取当作可信使用（如拼 SQL / 转发）
    static String consume(Session session) {
        String data = (String) session.getAttribute("trustedData");
        return "forward:" + data; // 误当可信
    }
}
