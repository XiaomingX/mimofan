package blinded;

import java.io.InputStream;
import java.net.URI;
import java.net.URL;
import java.security.PublicKey;
import java.util.Map;
import java.util.Set;

/*
 * JSEF-Benchmark L3 — JWT jku 域名边界校验修复（CWE-345）
 *
 * 修复：用 URI.getHost() 解析 host，精确匹配或 endsWith(".trust.issuer.com")
 * 域名边界匹配（"." 前缀杜绝子域名后缀绕过），且 kid 必须命中本地钉扎白名单。
 *
 * CWE-345 (Insufficient Verification of Data Authenticity)。
 */
public class JwtJkuSuffixBy {

    private static final String TRUSTED_HOST = "trust.issuer.com";
    private static final Set<String> PINNED_KIDS = Set.of("local-kid-001");

    public PublicKey fetchJwks(String jwksUrl) throws Exception {
        // 仅当 host 校验通过才访问
        try (InputStream is = new URL(jwksUrl).openStream()) {
            return parsePubKey(is);
        }
    }

    public boolean verify(String token, Map<String, Object> header) throws Exception {
        String jku = (String) header.get("jku");
        String kid = (String) header.get("kid");
        if (!isTrustedHost(jku) || !PINNED_KIDS.contains(kid)) { // 域名边界 + kid 钉扎
            return false;
        }
        PublicKey pk = fetchJwks(jku); // host 校验通过后拉取 JWKS
        /*ANCHOR_1*/
        return verifyWith(pk, token); // 可信公钥验签
    }

    private static boolean isTrustedHost(String jku) {
        try {
            String host = URI.create(jku).getHost();
            return host != null && (host.equals(TRUSTED_HOST) || host.endsWith("." + TRUSTED_HOST));
        } catch (Exception e) {
            return false;
        }
    }

    static PublicKey parsePubKey(InputStream is) { return null; }
    static boolean verifyWith(PublicKey pk, String token) { return true; }
}
