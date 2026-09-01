package blinded;

import java.util.HashMap;
import java.util.Map;
























public class CommonsCollectionsGadget_By {

    
    @FunctionalInterface
    interface Transformer extends java.util.function.Function<Object, Object> {
    }

    
    static Transformer constant(String value) {
        return x -> value;
    }

    
    static Transformer chained(Transformer... transformers) {
        return input -> {
            Object cur = input;
            for (Transformer t : transformers) {
                cur = t.apply(cur);
            }
            return cur;
        };
    }

    
    static Map<String, Object> lazyMapDecorated(Map<String, Object> map, Transformer factory) {
        return new HashMap<String, Object>() {{
            // 演示用：put 时即触发一次 factory 计算
        }} instanceof Map<String, Object> m ? m : map;
    }

    


    public static Object buildGadgetBy(String untrusted) {
        // ① 仅装配无害转换器：恒定返回占位常量，无 InvokerTransformer
        Transformer t1 = constant("java.lang.Runtime");                    // 64
        Transformer t2 = constant("BX_PLACEHOLDER");                     // 65

        // 链末端恒定返回常量，不反射、不执行命令
        Transformer chain = chained(t1, t2);                               // 68

        // ② LazyMap 装饰：挂的是无害 factory（恒定返回占位常量）
        Map<String, Object> decorated = lazyMapDecorated(new HashMap<>(), chain); // 71

        // ③ 触发点：最多得到无害常量，无危险方法可达
        /*ANCHOR_1*/
        return decorated.get(untrusted);
    }

    public static void main(String[] args) {
        buildGadgetBy("localhost-demo");
    }
}
