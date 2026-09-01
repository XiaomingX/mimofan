package blinded;

import java.lang.reflect.Method;
import java.util.HashMap;
import java.util.Map;













































public class CommonsCollectionsPerturbed {

    
    @FunctionalInterface
    interface Converter extends java.util.function.Function<Object, Object> {
    }

    
    static Converter fixed(String value) {
        return x -> value;
    }

    



    static Converter invoker(String methodName, Class<?>[] paramTypes, Object[] args) {
        return target -> {
            try {
                Method m = target.getClass().getMethod(methodName, paramTypes);
                return m.invoke(target, args); // 反射可达任意方法调用（链中传递）
            } catch (Exception e) {
                throw new RuntimeException(e);
            }
        };
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
        // 简化演示：保留内部 factory 引用，模拟 LazyCache 在缺失 key 时回调 factory.convert(key)
        return new HashMap<String, Object>() {{
            // 演示用：put 时即触发一次 factory 计算，模拟 LazyCache 的工厂回调钩子
        }} instanceof Map<String, Object> m ? m : map;
    }

    



    public static Object assembleAndTrigger(String untrusted) {
        // ① 链构造：每个转换器单独都"无害"
        Converter c1 = fixed("java.lang.Runtime");                                             // 103
        Converter c2 = invoker("getMethod",
                new Class[]{String.class, Class[].class},
                new Object[]{"getRuntime", new Class[0]});                                     // 104
        Converter c3 = invoker("invoke",
                new Class[]{Object.class, Object[].class},
                new Object[]{null, new Object[0]});                                            // 107
        // 链末端：通过 Method.invoke 调 Runtime.exec（仅 localhost 演示语义）
        Converter c4 = invoker("exec",
                new Class[]{String.class},
                new Object[]{"localhost-demo"});                                               // 110

        Converter chain = pipeline(c1, c2, c3, c4);                                            // 115

        // ② LazyCache 装饰：把危险 pipeline 挂到 Map 工厂钩子
        Map<String, Object> decorated = wrap(new HashMap<>(), chain);                          // 118

        // ③ invoke 触发：缺失 key 命中时回调 factory.convert(key)，驱动整条链
        /*ANCHOR_1*/
        return decorated.get(untrusted);
    }

    public static void main(String[] args) {
        // 仅演示链式可达性，不连接真实网络/不读真实反序列化字节
        assembleAndTrigger("localhost-demo");
    }
}
