/*
 * JSEF Benchmark 样本 — 信任边界安全对照 (CWE-501, L3)
 * 存入会话前校验，读取时再次校验。
 * 安全底线：仅 localhost 演示语义。
 */
package blinded;

import java.util.regex.Pattern;

public class TrustBoundaryViolationBy {

    interface Session { void setAttribute(String k, Object v); Object getAttribute(String k); }

    private static final Pattern BX = Pattern.compile("^[a-zA-Z0-9_\\-]+$");

    static void store(Session session, String userInput) {
        if (!BX.matcher(userInput).matches()) {
            throw new IllegalArgumentException("invalid input");
        }
        /*ANCHOR_1*/
        session.setAttribute("trustedData", userInput);
    }

    static String consume(Session session) {
        String data = (String) session.getAttribute("trustedData");
        if (!BX.matcher(data).matches()) {
            throw new IllegalArgumentException("invalid trusted data");
        }
        return "forward:" + data;
    }
}
