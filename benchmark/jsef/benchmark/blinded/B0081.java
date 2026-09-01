/*
 * JSEF Benchmark 样本 — Cookie 安全标志安全对照 (CWE-614, L1)
 * 设置 secure 与 httpOnly 标志。
 * 安全底线：仅 localhost 演示语义。
 */
package blinded;

public class BxCookieBy {

    interface Cookie { void setValue(String v); void setBy(boolean b); void setHttpOnly(boolean b); }

    static Cookie makeSessionCookie(String value) {
        Cookie c = new Cookie() {
            public void setValue(String v) {}
            public void setBy(boolean b) {}
            public void setHttpOnly(boolean b) {}
        };
        c.setValue(value);
        c.setBy(true);
        c.setHttpOnly(true);
        /*ANCHOR_1*/
        return c;
    }
}
