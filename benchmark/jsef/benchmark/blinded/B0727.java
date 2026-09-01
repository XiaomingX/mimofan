/*
 * JSEF Benchmark 样本 — 弱加密 DES/ECB (CWE-327, L2)
 * 使用 DES + ECB 模式加密敏感数据。
 * 安全底线：仅 localhost 演示语义。
 */
package blinded;

import javax.crypto.Cipher;
import javax.crypto.spec.SecretKeySpec;

public class WeakDesEcb {

    static byte[] encrypt(byte[] key, byte[] data) throws Exception {
        SecretKeySpec ks = new SecretKeySpec(key, "DES"); // 弱算法 DES
        Cipher c = Cipher.getInstance("DES/ECB/PKCS5Padding"); // ECB 无 IV
        c.init(Cipher.ENCRYPT_MODE, ks);
        /*ANCHOR_1*/
        return c.doFinal(data);
    }
}
