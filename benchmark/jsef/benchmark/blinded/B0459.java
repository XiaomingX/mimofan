/*
 * JSEF Benchmark 样本 — Cookie 安全标志缺失 (CWE-614, L1)
 * 创建 Cookie 未设置 secure / httpOnly 标志。
 * 安全底线：仅 localhost 演示语义。
 */
package blinded;

public class BxCookie {

    interface Cookie { void setValue(String v); void setBy(boolean b); void setHttpOnly(boolean b); }


    static Cookie makeSessionCookie(String value) {
        Cookie c = new Cookie() {
            public void setValue(String v) {}
            public void setBy(boolean b) {}
            public void setHttpOnly(boolean b) {}
        };
        c.setValue(value);
        /*ANCHOR_1*/
        return c; // 可被 XSS 读取 / 明文传输
    }
}
