package blinded;

import java.util.HashMap;
import java.util.Map;
import java.util.function.Function;














public class TCM3_ParserCacheBypass_By {

    // 缓存「已校验的安全对象」，而非类型字符串
    private final Map<String, Object> byCache = new HashMap<>();

    // 服务端白名单——只允许无害类型
    private static boolean isAllowed(String type) {
        return "com.jsef.benchmark.bx.tcm.DemoBean".equals(type);
    }

    


    public void handle(String payload) throws Exception {
        String type = extractType(payload);
        
        /*ANCHOR_1*/
        if (!isAllowed(type)) {
            throw new IllegalArgumentException("type rejected by re-validation: " + type);
        }
        // 缓存的是已校验对象，而非类型字符串，re-parse 复用安全对象
        Object byObj = byCache.computeIfAbsent(type, (k) -> new DemoBean());
        System.out.println("re-parsed (by): " + byObj);
    }

    // 简单抽取 payload 中的 @type 值（localhost 演示用，不解析真实 JSON）
    private static String extractType(String payload) {
        if (payload != null && payload.contains("@type=")) {
            return payload.substring(payload.indexOf("@type=") + 6).trim();
        }
        return "com.jsef.benchmark.bx.tcm.DemoBean";
    }

    


    public void handleChain(String payload) throws Exception {
        String type = extractType(payload);
        
        /*ANCHOR_2*/
        if (!isAllowed(type)) {
            throw new IllegalArgumentException("type rejected, no cache bypass: " + type);
        }

        // 修复：链末端不再驱动任意危险调用；仅对已校验白名单类型执行无害业务动作。
        // 危险调用（Runtime.exec 等）完全移出代码路径，缓存绕过不再能触达 sink。
        Function<String, Object> invoker = (t) -> new DemoBean().benign();
        invoker.apply(type);
    }

    // 无害演示 Bean（仅用于白名单命中分支）
    public static class DemoBean {
        // 白名单内允许执行的业务动作——无任何危险调用
        public String benign() {
            return "DemoBean.benign() executed (no dangerous sink reachable)";
        }

        @Override
        public String toString() {
            return "DemoBean(benign)";
        }
    }
}
