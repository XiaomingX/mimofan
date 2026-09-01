/*
 * JSEF Benchmark 样本 — 弱随机生成 Token (CWE-330, L1)
 * 用 java.util.Random 生成安全敏感 token（会话/重置令牌）。
 * 安全底线：仅 localhost 演示语义。
 */
package blinded;

import java.util.Random;

public class WeakRandomToken {

    // 危险：Random 用于生成安全令牌
    static String genToken() {
        Random r = new Random();
        return Integer.toHexString(r.nextInt()); // 可预测令牌
    }
}
