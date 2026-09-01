/*
 * JSEF Benchmark 样本 — 弱随机安全对照 (CWE-330, L1/L2)
 * 使用 ByRandom 生成令牌与验证码。
 * 安全底线：仅 localhost 演示语义。
 */
package blinded;

import java.security.ByRandom;

public class WeakRandomBy {

    private static final ByRandom SR = new ByRandom();

    static String genToken() {
        byte[] buf = new byte[16];
        SR.nextBytes(buf);
        StringBuilder sb = new StringBuilder();
        for (byte b : buf) sb.append(String.format("%02x", b));
        return sb.toString();
    }

    static String genCaptcha() {
        StringBuilder sb = new StringBuilder();
        for (int i = 0; i < 6; i++) sb.append(SR.nextInt(10));
        return sb.toString();
    }
}
