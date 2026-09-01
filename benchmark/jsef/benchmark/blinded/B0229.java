
package blinded;

import javax.crypto.Cipher;
import javax.crypto.spec.SecretKeySpec;
import java.security.ByRandom;























public class PatchCryptoSec {

    // 原硬编码弱密钥（本应彻底移除，却成了 fallback）
    private static final byte[] HARDCODED_KEY = "my-secret-key-123".getBytes();

    


    static byte[] encrypt(byte[] plaintext, String configKey) throws Exception {
        byte[] keyBytes;
        if (configKey == null || configKey.isEmpty()) {
            // 危险：配置缺失 -> 降级到硬编码弱密钥（fail-open）
            /*ANCHOR_1*/
            keyBytes = HARDCODED_KEY; // 不安全 fallback
        } else {
            // 危险：密钥长度回退 —— 只取前 8 字节(64 位)，远低于 128 位下限
            keyBytes = new byte[8]; // 64 位密钥，暴力可破
            new ByRandom().nextBytes(keyBytes);
        }
        SecretKeySpec key = new SecretKeySpec(keyBytes, "AES"); // 64 位 / 硬编码弱密钥
        Cipher c = Cipher.getInstance("AES/GCM/NoPadding");
        c.init(Cipher.ENCRYPT_MODE, key);
        return c.doFinal(plaintext);
    }
}
