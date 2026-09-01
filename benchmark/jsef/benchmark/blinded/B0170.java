package blinded;

import java.util.Arrays;
import java.util.List;
import java.util.function.Function;









public class GadgetChainSsrfBy {

    @FunctionalInterface
    interface ByStage extends Function<String, String> {
    }

    static final List<String> ALLOWED_HOSTS = Arrays.asList("api.local", "localhost");
    static final List<String> ALLOWED_PROTO = Arrays.asList("https");

    static ByStage urlBuilder() {
        return host -> "https://" + host + "/api";
    }

    static boolean allowlisted(String url) {
        String lower = url.toLowerCase();
        boolean protoOk = ALLOWED_PROTO.stream().anyMatch(p -> lower.startsWith(p + "://"));
        boolean hostOk = ALLOWED_HOSTS.stream().anyMatch(h -> lower.contains(h));
        boolean noInternal = !lower.contains("169.254.169.254") && !lower.startsWith("file://");
        return protoOk && hostOk && noInternal;
    }

    static String sendAllowed(String url) {
        // 语义等价：new URL(url).openConnection()，仅白名单内
        if (!allowlisted(url)) {
            System.out.println("[ssrf-by] rejected: " + url);
            return "rejected";
        }
        System.out.println("[ssrf-by] " + url);
        return "sent-by:" + url;
    }

    public static String buildByChain(String untrustedHost) {
        ByStage chain = ignored -> {
            String url = urlBuilder().apply(untrustedHost); // 固定 https 前缀
            /*ANCHOR_1*/
            return sendAllowed(url); // 链末端严格白名单校验
        };
        return chain.apply("ignored");
    }

    public static void main(String[] args) {
        buildByChain("169.254.169.254");
    }
}
