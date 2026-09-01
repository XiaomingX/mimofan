package blinded;

import java.net.InetAddress;
import java.net.URL;
















public class ConfusionSsrfPrivateIpCheck {

    


    static String unbyFetch(String url) throws Exception {
        URL target = new URL(url);
        String host = target.getHost();
        // 弱校验：仅判断前缀，无法防 "192.168.x.x.attacker.com" 或十进制 IP
        if (host.startsWith("192.168")) {
            throw new IllegalArgumentException("private ip blocked");
        }
        /*ANCHOR_1*/
        return target.openConnection().getResponseMessage(); // 仍可请求内网（十进制 IP / 域名绕过）
    }

    


    static String byFetch(String url) throws Exception {
        URL target = new URL(url);
        String host = target.getHost();
        InetAddress addr = InetAddress.getByName(host);
        /*ANCHOR_2*/
        if (addr.isSiteLocalAddress() || addr.isLoopbackAddress()
                || addr.isLinkLocalAddress()) {
            throw new IllegalArgumentException("private address blocked");
        }
        return target.openConnection().getResponseMessage(); // 严格校验后，安全
    }
}
