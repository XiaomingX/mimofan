package blinded;

import java.net.InetAddress;
import java.net.InetSocketAddress;
import java.net.Socket;
import java.net.URL;
















public class BlindSsrfNoResponseBy {

    static String probe(String url) throws Exception {
        URL target = new URL(url);
        // 一次性解析：后续直接使用此 IP，不再重新解析（防 DNS rebinding）
        InetAddress addr = InetAddress.getByName(target.getHost());

        /*ANCHOR_1*/
        if (isPrivateOrLocalAddress(addr)) {
            throw new IllegalArgumentException("private/local address blocked: " + addr.getHostAddress());
        }

        int port = target.getPort() > 0 ? target.getPort() : target.getDefaultPort();
        // 直接对已校验的 IP 建立 Socket（不经过 DNS 二次解析，消除 rebinding TOCTOU）
        try (Socket sock = new Socket()) {
            sock.connect(new InetSocketAddress(addr, port), 3000);
        }
        return "done";
    }

    
    private static boolean isPrivateOrLocalAddress(InetAddress addr) {
        if (addr.isSiteLocalAddress()) return true;   // 10/8、172.16/12、192.168/16、fc/fd::
        if (addr.isLoopbackAddress())  return true;   // 127.x、::1
        if (addr.isLinkLocalAddress()) return true;   // 169.254/16、fe80::/10
        if (addr.isAnyLocalAddress())  return true;   // 0.0.0.0 / ::
        // 额外检查：IPv4 0.0.0.0 通过数组判断（某些 JVM isSiteLocal 不覆盖）
        byte[] raw = addr.getAddress();
        if (raw.length == 4 && raw[0] == 0 && raw[1] == 0 && raw[2] == 0 && raw[3] == 0) return true;
        return false;
    }
}
