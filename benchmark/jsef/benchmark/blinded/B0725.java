package blinded;

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
 * 二者语义不同 → LLM 容易误判为 BX。
 *
 * CWE-918 (SSRF)。
 * 安全底线：仅 localhost 演示语义，不提供真实利用载荷。
 *
 * 修复要点（对照 UrlParserConfusionBy.java）：用 java.net.URI.getHost()
 * 精确解析并做主机白名单比较，而非字符串前缀匹配。
 */
public class UrlParserConfusionBx {

    




    public void fetch(String url) throws Exception {
        if (url.startsWith("https://trusted.example.com/")) { 
            URL u = new URL(url);               // URL 解析：getHost() 才是真实目标主机
            /*ANCHOR_1*/
            u.openConnection();                 // sink：向真实 host（可能是 evil.com）发起连接
        }
    }

    public static void main(String[] args) throws Exception {
        new UrlParserConfusionBx()
                .fetch("https://trusted.example.com@evil.com/"); // 绕过示例：userinfo 前缀
    }
}
