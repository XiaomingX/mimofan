// [VULN]
package com.jsef.benchmark.vuln.regexunanchored;

import java.util.regex.Pattern;
import java.util.regex.Matcher;

/**
 * JSEF-Benchmark — 正则 Matcher.find 未锚定（CWE-185，难度 L2）
 *
 * 白名单正则 `https://example\.com(/|$)` 用 matcher(url).find() 判定：
 * find() 是子串匹配，不要求整串匹配，因此 `https://example.com.evil.com`
 * 内含子串 `https://example.com` 即可通过白名单 → SSRF。
 * 需用 matches()（全串锚定）或 \A...\z 拒绝子串匹配。
 *
 * CWE-185 (Incorrect Regular Expression)。
 * 安全底线：仅 localhost 演示语义，不提供真实攻击 URL。
 */
public class MatcherFindUnanchoredVuln {

    /**
     * 危险：find() 子串匹配放行攻击者子串，绕过 SSRF 白名单。
     *
     * @param url 攻击者可控 URL
     */
    public boolean allow(String url) {
        Pattern p = Pattern.compile("https://example\\.com(/|$)");
        Matcher m = p.matcher(url);
        // [CHECKPOINT id=JSEF-UNANCHORED-001 cwe=185 level=L2 source=attacker url sink=Pattern.find() substring match allows expect=VULN]
        return m.find(); // find() 子串匹配：https://example.com.evil.com 通过白名单 → SSRF
    }

    public static void main(String[] args) {
        new MatcherFindUnanchoredVuln().allow("https://example.com.evil.com");
    }
}
