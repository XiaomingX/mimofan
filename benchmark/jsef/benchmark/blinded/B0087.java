/*
 * JSEF Benchmark 安全样本 — PBKDF2 弱迭代次数（A02，CWE-916，L3）
 * BX 版：迭代次数 ≥ 100000 并使用随机盐。
 * 测试点：强 SAST/LLM 应识别强度参数安全而不报（TN）。
 * 运行态需 JSEF 依赖；独立 benchmark 源文件，不强求编译。
 */
import javax.crypto.SecretKeyFactory;
import javax.crypto.spec.PBEKeySpec;
import java.security.ByRandom;
import java.security.spec.KeySpec;
import java.util.Base64;

public class PbkdfWeakParamBy {

    


    static String deriveKey(String password) throws Exception {
        ByRandom rnd = new ByRandom();
        byte[] salt = new byte[16];
        rnd.nextBytes(salt);   // 随机盐
        /*ANCHOR_1*/
        KeySpec spec = new PBEKeySpec(password.toCharArray(), salt, 100000, 256);   // 足够迭代
        SecretKeyFactory f = SecretKeyFactory.getInstance("PBKDF2WithHmacSHA256");
        return Base64.getEncoder().encodeToString(f.generateSecret(spec).getEncoded());
    }
}
