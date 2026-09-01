package blinded;

import java.util.HashMap;
import java.util.Map;
import java.util.function.Function;

















public class TCM3_ParserCacheBypass {

    // 模拟解析器缓存（真实漏洞里是反序列化器 / 类型缓存）
    private final Map<String, String> typeCache = new HashMap<>();

    // 服务端的「白名单」——只允许无害类型
    private static boolean isAllowed(String type) {
        return "com.jsef.benchmark.bz.tcm.DemoBean".equals(type);
    }

    




    public void handle(String payload) throws Exception {
        // 第一次解析：抽取 @type，命中白名单校验后拒绝
        
        typeCache.put("@type", extractType(payload)); // 行：缓存写入（绕过起点）

        // 第二次 reParse：直接复用缓存，不再校验
        String cachedType = typeCache.get("@type");
        Class<?> c = Class.forName(cachedType);
        /*ANCHOR_1*/
        Object obj = c.newInstance(); // 缓存绕过后的实例化，触发隐式初始化 sink
        System.out.println("re-parsed: " + obj);
    }

    // 简单抽取 payload 中的 @type 值（localhost 演示用，不解析真实 JSON）
    private static String extractType(String payload) {
        // 演示：假设 payload 形如 "@type=evil.Class"
        if (payload != null && payload.contains("@type=")) {
            return payload.substring(payload.indexOf("@type=") + 6).trim();
        }
        return "com.jsef.benchmark.bz.tcm.DemoBean";
    }

    






    public void handleChain(String payload) throws Exception {
        // 缓存写入（与 L3 同一缺陷起点）
        
        typeCache.put("@type", extractType(payload)); // 行：缓存写入（链起点）

        String cachedType = typeCache.get("@type");

        // 仿 Transformer：根据类型名返回一个执行器 Function
        Function<String, Object> invoker = (type) -> {
            try {
                // 反射拿到 Runtime 并 exec（localhost-demo 占位）
                Class<?> rt = Class.forName("java.lang.Runtime");
                java.lang.reflect.Method getRuntime = rt.getMethod("getRuntime");
                Object runtime = getRuntime.invoke(null);
                
                java.lang.reflect.Method exec = rt.getMethod("exec", String.class);
                /*ANCHOR_2*/
                return exec.invoke(runtime, "localhost-demo"); // 仅占位，不连真实远端
            } catch (Exception e) {
                throw new RuntimeException(e);
            }
        };

        // 链末端触发：缓存绕过后的类型驱动反射 exec
        invoker.apply(cachedType);
    }

    // 无害演示 Bean（仅用于白名单命中分支）
    public static class DemoBean {
        @Override
        public String toString() {
            return "DemoBean(benign)";
        }
    }
}
