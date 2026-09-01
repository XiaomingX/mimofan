package blinded;

import java.net.HttpURLConnection;
import java.net.URL;

/*
 * JSEF-Benchmark L3 — SSRF 重定向安全对照
 *
 * 修复：setInstanceFollowRedirects(false) 不跟随重定向，避免跳转到内网。
 * BX 侧按实现判定安全。
 */
public class SsrfRedirectBy {

    public void run(String userUrl) throws Exception {
        if (!isWhitelisted(userUrl)) {
            throw new IllegalArgumentException("host not allowed");
        }
        URL url = new URL(userUrl);
        HttpURLConnection conn = (HttpURLConnection) url.openConnection();
        conn.setInstanceFollowRedirects(false);
        /*ANCHOR_1*/
        conn.getInputStream();
    }

    static boolean isWhitelisted(String u) {
        return u.startsWith("https://example.com/");
    }

    public static void main(String[] args) throws Exception {
        new SsrfRedirectBy().run("https://example.com/api");
    }
}
