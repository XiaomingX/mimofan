package blinded;

import java.util.HashMap;
import java.util.Map;


























public class CommonsCollectionsPerturbed_By {

    
    @FunctionalInterface
    interface Converter extends java.util.function.Function<Object, Object> {
    }

    
    static Converter fixed(String value) {
        return x -> value;
    }

    
    static Converter pipeline(Converter... converters) {
        return input -> {
            Object cur = input;
            for (Converter c : converters) {
                cur = c.apply(cur);
            }
            return cur;
        };
    }

    
    static Map<String, Object> wrap(Map<String, Object> map, Converter factory) {
        return new HashMap<String, Object>() {{
            // 演示用：put 时即触发一次 factory 计算
        }} instanceof Map<String, Object> m ? m : map;
    }

    


    public static Object assembleAndTriggerBy(String untrusted) {
        // ① 仅装配无害转换器：恒定返回占位常量，无 MethodInvoker
        Converter c1 = fixed("java.lang.Runtime");                    // 64
        Converter c2 = fixed("BX_PLACEHOLDER");                     // 65

        // 链末端恒定返回常量，不反射、不执行命令
        Converter chain = pipeline(c1, c2);                           // 68

        // ② LazyCache 装饰：挂的是无害 factory（恒定返回占位常量）
        Map<String, Object> decorated = wrap(new HashMap<>(), chain); // 71

        // ③ 触发点：最多得到无害常量，无危险方法可达
        /*ANCHOR_1*/
        return decorated.get(untrusted);
    }

    public static void main(String[] args) {
        assembleAndTriggerBy("localhost-demo");
    }
}
