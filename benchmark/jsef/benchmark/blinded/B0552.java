package blinded;

import java.util.function.Function;


















public class GadgetChainSsrf {

    @FunctionalInterface
    interface Stage extends Function<String, String> {
    }

    
    static Stage urlBuilder() {
        return host -> "http://" + host + "/api";
    }

    
    static Stage protocolNormalizer() {
        // 把 "Http://" 等前缀统一为小写；但同时把黑名单遗漏的大写协议也转小写
        return url -> url.replaceFirst("(?i)^(https?|file|javascript):", m -> m.group(0).toLowerCase());
    }

    
    static boolean isBlocked(String url) {
        // 黑名单仅小写匹配；被上一节点归一化后的 "JAVASCRIPT:" 已变 "javascript:"
        // 这里仅示例性列出内网网段，实际黑名单漏判大写
        return url.contains("169.254.169.254") || url.startsWith("file://");
    }

    
    static Stage httpSender() {
        return url -> {
            /*ANCHOR_1*/
            return send(url); // 归一化绕过黑名单后请求内网
        };
    }

    static String send(String url) {
        // 语义等价：new URL(url).openConnection()
        System.out.println("[ssrf-send] " + url);
        return "sent:" + url;
    }

    


    public static String buildAndTrigger(String untrustedHost) {
        Stage chain = ignored -> {
            String url = urlBuilder().apply(untrustedHost);   // URL 构造
            url = protocolNormalizer().apply(url);             // 协议归一化
            if (isBlocked(url)) {                              // 黑名单（被绕过）
                url = url.replace("http://", "file://");       // 演示：转入危险协议
            }
            return httpSender().apply(url);                    // 末端 sink
        };
        return chain.apply("ignored");
    }

    public static void main(String[] args) {
        buildAndTrigger("169.254.169.254/latest/meta-data");
    }
}
