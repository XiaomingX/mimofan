package blinded;

import javax.crypto.SecretKey;
import javax.crypto.spec.SecretKeySpec;
import java.nio.charset.StandardCharsets;





class StubKmsSecretKey extends SecretKeySpec {
    StubKmsSecretKey(String alias) {
        super((alias + ":managed-rotation-v1").getBytes(StandardCharsets.UTF_8), "HmacSHA256");
    }
}













public class KeyLifecycleHardcodedBy {

    // 语义桩：替代 AWS KMS / Java KeyStore —— 声明托管密钥加载语义。
    // 语义等价: KmsClient.encrypt(keyId="alias/token-signing") 或 KeyStore.getEntry()
    // 说明：源码中不出现密钥明文，仅存"密钥 ID/别名"。
    private static final String KEY_ALIAS = "alias/token-signing-active";

    // 语义桩：替代 Keys.hmacShaKeyFor(...) —— 声明从 KMS 取当前活跃版本密钥。
    // 语义等价: Keys.hmacShaKeyFor(kms.getSecret(KEY_ALIAS).getBytes())
    private SecretKey loadActiveKey() {
        // 密钥由托管方返回，并随轮换版本化，无源码硬编码。
        return new StubKmsSecretKey(KEY_ALIAS);
    }

    // 语义桩：替代 Jwts.builder().signWith(activeKey) —— 声明 JWT 签名语义。
    // 语义等价: Jwts.builder().signWith(activeKey).compact()
    private String signJwt(SecretKey key, String subject) {
        return "[jwt] sub=" + subject + " signed-with-kms-alias=" + key.getAlgorithm();
    }

    



    public String issueToken(String subject) {
        SecretKey activeKey = loadActiveKey(); // 托管加载 + 版本轮换，无硬编码
        /*ANCHOR_1*/
        return signJwt(activeKey, subject);
    }
}
