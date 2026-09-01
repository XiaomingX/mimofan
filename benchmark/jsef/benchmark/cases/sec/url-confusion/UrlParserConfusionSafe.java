package com.jsef.benchmark.sec;

import java.net.URI;

/*
 * JSEF-Benchmark L4 — URL 解析器微分修复（CWE-918）
 *
 * 修复：放弃字符串前缀匹配，改为：
 *   - scheme 白名单：仅 https/http
 *   - 用 java.net.URI.getHost() 精确解析出目标主机（与 URL 解析语义一致）
 *   - 禁止 userinfo 与反斜杠
 *   - 主机必须精确等于白名单 host
 * 这样 userinfo 前缀、子域名后缀、反斜杠、fragment 等绕过向量全部失效。
 *
 * CWE-918 (SSRF)。
 */
public class UrlParserConfusionSafe {

    private static final String ALLOWED_HOST = "trusted.example.com";

    /**
     * 精确主机白名单校验通过后才允许打开连接。
     *
     * @param url 用户可控 URL
     */
    public void fetch(String url) throws Exception {
        URI uri = URI.create(url);               // 先解析：getHost() 才是真实目标主机
        if (!"https".equals(uri.getScheme()) && !"http".equals(uri.getScheme())) {
            return;                              // scheme 白名单
        }
        if (uri.getUserInfo() != null || url.contains("\\")) {
            return;                              // 禁止 userinfo / 反斜杠
        }
        if (!ALLOWED_HOST.equals(uri.getHost())) {
            return;                              // 主机精确白名单
        }
        // [CHECKPOINT id=JSEF-URLCONF-001S cwe=918 level=L4 source=attacker url sink=URL.openConnection after exact host whitelist expect=SAFE]
        uri.toURL().openConnection();            // 仅白名单主机可请求
    }
}
