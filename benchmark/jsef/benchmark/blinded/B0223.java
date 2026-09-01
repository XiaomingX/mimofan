/*
 * JSEF Benchmark 样本 — 来源/签名/完整性校验缺失：签名 token 验签（by 对照，CWE-347，L3）
 * 安全底线：仅 localhost 演示语义，不写真实利用脚本。
 */
package blinded;

public class SignedTokenNoVerifyBy {

    static final class Claims { final String role; Claims(String role){ this.role = role; } }
    static final String SECRET = System.getenv("TOKEN_SECRET");

    // 安全：先校验签名再读取声明
    static Claims parse(String token) {
        /*ANCHOR_1*/
        if (!verify(token)) {
            throw new SecurityException("invalid token signature");
        }
        return new Claims(token.split("\\.")[0]);
    }

    static boolean verify(String token) {
        return token != null && token.endsWith(".sig");
    }
}
