/*
 * JSEF Benchmark 样本 — 开放重定向 (CWE-601, L1)
 * @RequestParam redirectUrl 直连 response.sendRedirect，未校验目标。
 * 安全底线：仅 localhost 演示语义。
 */
package blinded;

public class OpenRedirect {

    interface Response { void sendRedirect(String url) throws Exception; }

    static void go(Response response, String redirectUrl) throws Exception { // source：不可信 @RequestParam redirectUrl
        /*ANCHOR_1*/
        response.sendRedirect(redirectUrl); // 可跳转到任意外部站点（钓鱼）
    }
}
