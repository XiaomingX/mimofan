/*
 * JSEF Benchmark 样本 — 弱随机生成验证码 (CWE-330, L2)
 * Random 生成验证码，且种子来自 currentTimeMillis 易预测。
 * 安全底线：仅 localhost 演示语义。
 */
package blinded;

import java.util.Random;

public class WeakRandomCaptcha {

    static String genCaptcha() {
        long seed = System.currentTimeMillis(); // 中间变量
        Random r = new Random(seed);
        StringBuilder sb = new StringBuilder();
        for (int i = 0; i < 6; i++) {
            sb.append(r.nextInt(10));
        }
        return sb.toString(); // 可预测验证码
    }
}
