package blinded;

import java.net.InetAddress;
import java.net.URL;
import java.net.HttpURLConnection;

/*
 * JSEF-Benchmark L4 — SSRF DNS 重绑定安全对照
 *
 * 修复：解析后绑定 IP 连接，使用 addr.getHostAddress() 避免二次解析，
 * 校验点 == 连接点。
 * BX 侧按实现判定安全。
 */
public class SsrfRebindBy {

    public void run(String host) throws Exception {
        InetAddress addr = InetAddress.getByName(host);
        if (isInternal(addr)) {
            throw new IllegalArgumentException("internal host blocked");
        }
        URL url = new URL("http://" + addr.getHostAddress());  // 绑定已校验 IP
        /*ANCHOR_1*/
        HttpURLConnection conn = (HttpURLConnection) url.openConnection();
        conn.getInputStream();
    }

    static boolean isInternal(InetAddress addr) {
        byte[] ip = addr.getAddress();
        return ip[0] == 10 || (ip[0] == (byte) 192 && ip[1] == (byte) 168);
    }

    public static void main(String[] args) throws Exception {
        new SsrfRebindBy().run("example.com");
    }
}
