package blinded;

import java.util.Arrays;
import java.util.List;
import java.util.function.Function;









public class GadChainCmdBy {

    @FunctionalInterface
    interface ByProcessor extends Function<String, String> {
    }

    static ByProcessor constant(String prefix) {
        return x -> prefix;
    }

    static ByProcessor normalize() {
        return s -> s == null ? "" : s.trim().toLowerCase();
    }

    
    static String sanitize(String frag) {
        if (frag == null) return "";
        return frag.replaceAll("[^a-zA-Z0-9]", "");
    }

    static final List<String> ALLOWED = Arrays.asList("ls", "id", "date");

    static String execAllowed(String name) {
        // 语义等价：Runtime.getRuntime().exec(new String[]{name})，固定参数列表
        if (!ALLOWED.contains(name)) {
            System.out.println("[cmd-exec-by] rejected: " + name);
            return "rejected";
        }
        System.out.println("[cmd-exec-by] " + name);
        return "executed-by:" + name;
    }

    public static String buildByChain(String untrusted) {
        ByProcessor chain = ignored -> {
            String name = constant("").apply(null);            // 常量前缀
            name = normalize().apply(name + untrusted);         // 归一化
            name = sanitize(name);                              // 白名单截断
            /*ANCHOR_1*/
            return execAllowed(name); // 不可信仅作白名单校验后的名字，不拼 shell
        };
        return chain.apply("ignored");
    }

    public static void main(String[] args) {
        buildByChain("; id");
    }
}
