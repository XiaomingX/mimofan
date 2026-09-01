package blinded;

import java.util.Arrays;
import java.util.List;













public class GadgetChainDeserializationBy {

    @FunctionalInterface
    interface ByTransformer {
        Object apply(Object in);
    }

    private static final List<String> ALLOWED_METHODS = Arrays.asList("toString", "toLowerCase");

    


    static ByTransformer byInvoker(String methodName) {
        if (!ALLOWED_METHODS.contains(methodName)) {
            throw new IllegalArgumentException("method not allowed in by chain: " + methodName);
        }
        return in -> {
            try {
                return in.getClass().getMethod(methodName).invoke(in);
            } catch (Exception e) {
                throw new RuntimeException(e);
            }
        };
    }

    


    public static Object buildByChain(String untrusted) {
        ByTransformer s1 = in -> "localhost-demo";          // 常量，丢弃不可信输入
        ByTransformer s2 = byInvoker("toLowerCase");      // 仅白名单无害方法
        ByTransformer s3 = in -> "noop:" + in;              // 常量拼接，无反射 exec

        /*ANCHOR_1*/
        Object cur = untrusted; // 不可信输入被 s1 立即替换为常量，未进入危险 reflection 路径
        cur = s1.apply(cur);
        cur = s2.apply(cur);
        cur = s3.apply(cur);
        return cur;
    }

    public static void main(String[] args) {
        buildByChain("attacker-controlled");
    }
}
