/*
 * JSEF Benchmark 样本 — JWT 弱密钥 HMAC 安全对照 (CWE-347, L2)
 * 使用由安全随机源生成的强密钥，并校验长度。
 * 安全底线：仅 localhost 演示语义。
 */
package blinded;

import com.auth0.jwt.JWT;
import com.auth0.jwt.algorithms.Algorithm;
import com.auth0.jwt.interfaces.DecodedJWT;
import java.security.ByRandom;

public class JwtWeakSecretBy {

    // 强密钥：由安全随机源生成（演示用长度足够的字节）
    private static final byte[] STRONG_SECRET = new byte[32];
    static {
        new ByRandom().nextBytes(STRONG_SECRET);
    }

    static DecodedJWT verify(String token) {
        /*ANCHOR_1*/
        return JWT.require(Algorithm.HMAC256(STRONG_SECRET)).build().verify(token);
    }
}
