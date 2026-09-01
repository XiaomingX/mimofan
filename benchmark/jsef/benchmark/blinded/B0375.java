package blinded;

import java.lang.reflect.Method;
import java.util.HashMap;
import java.util.Map;
import java.util.function.Function;























public class GadgetChainDeserialization {

    
    @FunctionalInterface
    interface Transformer extends Function<Object, Object> {
    }

    
    static Transformer constant(String value) {
        return x -> value;
    }

    



    static Transformer invoker(String methodName, Class<?>[] paramTypes, Object[] args) {
        return target -> {
            try {
                Method m = target.getClass().getMethod(methodName, paramTypes);
                return m.invoke(target, args); // 反射可达任意方法调用
            } catch (Exception e) {
                throw new RuntimeException(e);
            }
        };
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

    



    static Map<String, Object> lazyMap(Map<String, Object> map, Transformer factory) {
        return new HashMap<String, Object>() {{
            // 简化演示：put 时即触发一次 factory 计算，模拟 LazyMap 的工厂回调钩子
        }} instanceof Map<String, Object> m ? m : map;
    }

    



    public static Object buildGadgetAndTrigger(String untrusted) {
        // 单独的转换器都"无害"：常量、反射调用工具
        Transformer t1 = constant("java.lang.Runtime");
        Transformer t2 = invoker("getMethod",
                new Class[]{String.class, Class[].class},
                new Object[]{"getRuntime", new Class[0]});
        Transformer t3 = invoker("invoke",
                new Class[]{Object.class, Object[].class},
                new Object[]{null, new Object[0]});
        // 危险 sink：通过 Method.invoke 调 Runtime.exec（仅 localhost 演示语义）
        Transformer t4 = invoker("exec",
                new Class[]{String.class},
                new Object[]{"echo localhost-demo"});

        Transformer chain = chained(t1, t2, t3, t4);

        // 不可信输入触发链：组合后形成 Runtime.exec 可达性
        /*ANCHOR_1*/
        return chain.apply(untrusted);
    }

    public static void main(String[] args) {
        // 仅演示链式可达性，不连接真实网络/不读真实反序列化字节
        buildGadgetAndTrigger("localhost-demo");
    }
}
