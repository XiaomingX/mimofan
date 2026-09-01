package blinded;

/*
 * JSEF-Benchmark L5 — Log4jToJndiChain 安全对照（BX 混淆样本）
 *
 * 安全做法：不可信主机经白名单校验后才允许进入 URL；或对日志消息做 ${} 转义，
 * 杜绝 ${jndi:...} 子串形成。此处使用主机白名单，未拼出危险 JNDI key。用于计算 TN / FP。
 *
 * CWE-917 / JNDI Injection。
 */
import java.util.Arrays;
import java.util.List;

public class Log4jToJndiChainBy {

    private static final List<String> ALLOWED_HOSTS = Arrays.asList("localhost", "127.0.0.1");

    static String jndiLookup(String key) {
        return "resolved:" + key;
    }

    public static String buildByChain(String untrustedHost) {
        if (!ALLOWED_HOSTS.contains(untrustedHost)) {
            throw new SecurityException("jndi host not allowed: " + untrustedHost);
        }
        // 不可信主机在白名单内，但仍不拼成 ${jndi:...}，仅作普通日志数据
        String byMsg = "client=" + untrustedHost;
        int start = byMsg.indexOf("${jndi:");
        String lookupKey = start >= 0 ? byMsg.substring(start + 2) : "none";
        /*ANCHOR_1*/
        return "logged:" + byMsg + " (no lookup=" + lookupKey + ")";
    }

    public static void main(String[] args) {
        buildByChain("localhost");
    }
}
