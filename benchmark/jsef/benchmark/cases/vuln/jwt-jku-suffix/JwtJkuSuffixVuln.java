package com.jsef.benchmark.vuln;

import java.io.InputStream;
import java.net.URL;
import java.security.PublicKey;
import java.util.Map;

/*
 * JSEF-Benchmark L3 — JWT jku 白名单后缀绕过（CWE-345）
 *
 * 难度：L3（跨方法）。信任校验使用 jku.startsWith(TRUSTED_DOMAIN)，
 * TRUSTED_DOMAIN="https://trust.issuer.com"。攻击者构造
 * "https://trust.issuer.com.evil.com/key.json"（子域名后缀）即可通过
 * 前缀校验，拉取攻击者控制的 JWKS 公钥，伪造 token 验签通过。
 *
 * CWE-345 (Insufficient Verification of Data Authenticity)。
 * 安全底线：仅 localhost 演示语义，不提供真实伪造 token。
 *
 * 修复要点（对照 JwtJkuSuffixSafe.java）：URI.getHost() 解析 host 做域名
 * 边界匹配 + kid 本地钉扎白名单。
 */
public class JwtJkuSuffixVuln {

    private static final String TRUSTED_DOMAIN = "https://trust.issuer.com";

    public PublicKey fetchJwks(String jwksUrl) throws Exception {
        // 从校验通过的 URL 拉取 JWKS——攻击者可控内容（SSRF + 公钥投毒）
        try (InputStream is = new URL(jwksUrl).openStream()) {
            return parsePubKey(is);
        }
    }

    public boolean verify(String token, Map<String, Object> header) throws Exception {
        String jku = (String) header.get("jku"); // 污点：jku 来自不可信 token 头
        if (!jku.startsWith(TRUSTED_DOMAIN)) { // 信任校验：前缀匹配，可被子域名后缀绕过
            return false;
        }
        PublicKey pk = fetchJwks(jku); // 拉取攻击者 JWKS
        // [CHECKPOINT id=JSEF-JKUSFX-001 cwe=345 level=L3 source=jku header startsWith bypass sink=JWT verify using attacker JWKS (trust.issuer.com.evil.com) expect=VULN trace=benchmark/cases/vuln/jwt-jku-suffix/JwtJkuSuffixVuln.java:35,benchmark/cases/vuln/jwt-jku-suffix/JwtJkuSuffixVuln.java:38,benchmark/cases/vuln/jwt-jku-suffix/JwtJkuSuffixVuln.java:40]
        return verifyWith(pk, token); // [VULN] 用不可信公钥验签，放行
    }

    static PublicKey parsePubKey(InputStream is) { return null; }
    static boolean verifyWith(PublicKey pk, String token) { return true; }
}
