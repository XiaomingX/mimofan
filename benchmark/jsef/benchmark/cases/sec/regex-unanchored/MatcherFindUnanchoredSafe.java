package com.jsef.benchmark.sec.regexunanchored;

import java.util.regex.Pattern;
import java.util.regex.Matcher;

/**
 * JSEF-Benchmark — 正则 Matcher.find 未锚定修复（CWE-185，难度 L2）
 *
 * 修复：用 matches()（全串锚定）替代 find()（子串匹配），
 * 或使用 \A...\z 显式锚定，拒绝 `https://example.com.evil.com`
 * 这类含白名单子串的攻击 URL，阻断 SSRF。
 *
 * CWE-185 (Incorrect Regular Expression)。
 */
public class MatcherFindUnanchoredSafe {

    /**
     * 安全：matches() 全串匹配，子串不再放行。
     *
     * @param url 用户可控 URL
     */
    public boolean allow(String url) {
        Pattern p = Pattern.compile("https://example\\.com(/|$)");
        Matcher m = p.matcher(url);
        // [CHECKPOINT id=JSEF-UNANCHORED-001S cwe=185 level=L2 source=attacker url sink=Pattern.matches() full-string anchor reject expect=SAFE]
        return m.matches(); // matches() 全串锚定：https://example.com.evil.com 被拒
    }

    public static void main(String[] args) {
        new MatcherFindUnanchoredSafe().allow("https://example.com.evil.com");
    }
}
