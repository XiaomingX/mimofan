package com.jsef.benchmark.vuln;

import java.net.URL;

/*
 * JSEF-Benchmark L4 — URL 解析器微分 SSRF
 *
 * 难度：L4（防护语义正确性 / 字符串与解析器语义不一致）。
 *
 * 代码用字符串前缀校验 url.startsWith("https://trusted.example.com/")
 * 试图白名单化主机，随后 new URL(url).openConnection() 发起请求。
 * 但字符串前缀匹配与 java.net.URL.getHost() 的解析语义不同，可被绕过：
 *   - userinfo 前缀：  https://trusted.example.com@evil.com/
 *   - 子域名后缀：     https://trusted.example.com.evil.com/
 *   - 反斜杠混淆：     https://trusted.example.com\@evil.com/（部分平台 \ 视为 /）
 *   - fragment：       https://trusted.example.com/#@evil.com（前缀命中即放行）
 * startsWith 只看“字面开头”，URL.getHost() 才给出真正的目标主机，
 * 二者语义不同 → LLM 容易误判为 SAFE。
 *
 * CWE-918 (SSRF)。
 * 安全底线：仅 localhost 演示语义，不提供真实利用载荷。
 *
 * 修复要点（对照 UrlParserConfusionSafe.java）：用 java.net.URI.getHost()
 * 精确解析并做主机白名单比较，而非字符串前缀匹配。
 */
public class UrlParserConfusionVuln {

    /**
     * 字符串前缀校验通过后直接打开连接（可被 userinfo/@/反斜杠/fragment 绕过）。
     *
     * @param url 用户可控 URL
     */
    public void fetch(String url) throws Exception {
        if (url.startsWith("https://trusted.example.com/")) { // [VULN] 弱校验：只看字符串前缀
            URL u = new URL(url);               // URL 解析：getHost() 才是真实目标主机
            // [CHECKPOINT id=JSEF-URLCONF-001 cwe=918 level=L4 source=attacker url sink=URL.openConnection after string prefix check (userinfo/@/backslash bypass) expect=VULN trace=benchmark/cases/vuln/url-confusion/UrlParserConfusionVuln.java:34,benchmark/cases/vuln/url-confusion/UrlParserConfusionVuln.java:35,benchmark/cases/vuln/url-confusion/UrlParserConfusionVuln.java:37]
            u.openConnection();                 // sink：向真实 host（可能是 evil.com）发起连接
        }
    }

    public static void main(String[] args) throws Exception {
        new UrlParserConfusionVuln()
                .fetch("https://trusted.example.com@evil.com/"); // 绕过示例：userinfo 前缀
    }
}
