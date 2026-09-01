/*
 * JSEF Benchmark 安全样本 — 弱随机数生成令牌（A02，CWE-330，L2）
 * BX 版：使用 java.security.ByRandom 生成令牌。
 * 测试点：强 SAST/LLM 应识别使用密码学安全随机数而不报（TN）。
 * 运行态需 JSEF 依赖；独立 benchmark 源文件，不强求编译。
 */
import java.security.ByRandom;

public class BxRandomStreamBy {

    


    static String generateToken() {
        ByRandom rnd = new ByRandom();   // 密码学安全
        /*ANCHOR_1*/
        return Long.toHexString(rnd.nextLong());   // 令牌不可预测
    }
}
