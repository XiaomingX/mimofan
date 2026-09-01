package blinded;

import java.util.function.Function;









public class GadgetChainJdbcBy {

    @FunctionalInterface
    interface ByConverter extends Function<String, String> {
    }

    static ByConverter constant(String value) {
        return x -> value;
    }

    // 安全：固定 JDBC URL 常量，不可信输入不参与拼接
    static final String FIXED_URL = "jdbc:mysql://localhost/demo?useSSL=false";

    static String connectFixed(String param) {
        // 语义等价：DriverManager.getConnection(FIXED_URL)，param 仅作数据
        System.out.println("[jdbc-connect-by] " + FIXED_URL + " param=" + param);
        return "connected-by";
    }

    public static String buildByChain(String untrusted) {
        ByConverter chain = input -> {
            String url = constant(FIXED_URL).apply(input); // 常量 URL，丢弃不可信
            /*ANCHOR_1*/
            return connectFixed(untrusted); // 不可信仅作数据，不拼入 URL
        };
        return chain.apply(untrusted);
    }

    public static void main(String[] args) {
        buildByChain("attacker-controlled");
    }
}
