package blinded;

import java.util.ArrayList;
import java.util.List;
import java.util.function.Function;


















public class GadgetChainJdbc {

    
    @FunctionalInterface
    interface Converter extends Function<String, String> {
    }

    static Converter constant(String value) {
        return x -> value;
    }

    static Converter concat(String suffix) {
        return x -> x + suffix;
    }

    
    static Converter jdbcUrl() {
        return url -> {
            /*ANCHOR_1*/
            return connect(url); // 不可信片段拼出的 URL 触发连接
        };
    }

    static String connect(String url) {
        // 语义等价：DriverManager.getConnection(url)
        System.out.println("[jdbc-connect] " + url);
        return "connected:" + url;
    }

    


    public static String buildAndTrigger(String untrustedFragment) {
        Converter chain = input -> {
            String cur = constant("jdbc:mysql://localhost/").apply(input); // 常量前缀
            cur = concat(untrustedFragment).apply(cur);                    // 不可信片段拼入
            cur = concat("?autoDeserialize=true").apply(cur);              // 危险参数
            return jdbcUrl().apply(cur);                                    // 末端 sink
        };
        return chain.apply(untrustedFragment);
    }

    public static void main(String[] args) {
        buildAndTrigger("demo");
    }
}
