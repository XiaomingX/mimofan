
package blinded;

import javax.crypto.Cipher;
import javax.crypto.spec.SecretKeySpec;













public class PatchCryptoBx {

    // 危险：硬编码密钥 + 仅 16 字节（128 位但 ECB 模式仍不安全）
    private static final byte[] HARDCODED_KEY = "my-secret-key-123".getBytes();

    


    static byte[] encrypt(byte[] plaintext) throws Exception {
        SecretKeySpec key = new SecretKeySpec(HARDCODED_KEY, "AES");
        /*ANCHOR_1*/
        Cipher c = Cipher.getInstance("AES/ECB/PKCS5Padding"); // ECB 模式不安全
        c.init(Cipher.ENCRYPT_MODE, key);
        return c.doFinal(plaintext);
    }
}
