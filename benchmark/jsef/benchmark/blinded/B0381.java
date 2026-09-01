package blinded;

import javax.crypto.Mac;
import javax.crypto.spec.SecretKeySpec;
import java.nio.charset.StandardCharsets;














public class KeyLifecycleHardcodedBx {

    // 硬编码密钥源：明文出现在源码常量，任何人可读取并复制出有效签名密钥。
    private static final String HMAC_SECRET = "p@ssw0rd-static-hmac-key-2024";

    // 语义桩：替代 JwtSecretKey / Keys.hmacShaKeyFor —— 声明签名语义。
    // 语义等价: Keys.hmacShaKeyFor(HMAC_SECRET.getBytes())
    private SecretKeySpec loadSigningKey() {
        return new SecretKeySpec(HMAC_SECRET.getBytes(StandardCharsets.UTF_8), "HmacSHA256");
    }

    // 语义桩：替代 Jwts.builder().signWith(secretKey) —— 声明 JWT 签名语义。
    // 语义等价: Jwts.builder().signWith(secretKey).compact()
    private String signJwt(SecretKeySpec key, String subject) {
        return "[jwt] sub=" + subject + " signed-with=" + key.getAlgorithm();
    }

    



    public String issueToken(String subject) {
        SecretKeySpec secretKey = loadSigningKey(); // 密钥来自硬编码常量
        /*ANCHOR_1*/
        return signJwt(secretKey, subject);
    }
}
