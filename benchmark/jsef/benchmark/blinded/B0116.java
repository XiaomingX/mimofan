package blinded;











public class GmNoDefaultCtorBy {

    
    private static final java.util.Set<String> ALLOWLIST = java.util.Set.of(
            "com.jsef.benchmark.bx.gadgetmine.GmNoDefaultCtorBy.ByModel"
    );

    


    public static class ByModel {
        // public 无参构造：实例化入口可用
        public ByModel() {
            // 安全默认构造
        }
    }

    


    public static Object byResolveViaType(String typeName) {
        /*ANCHOR_1*/
        if (!ALLOWLIST.contains(typeName)) {   // 白名单校验：阻断任意类实例化
            throw new IllegalArgumentException("type not allowed: " + typeName);
        }
        System.out.println("[demo-only] by-instantiate allowed type: " + typeName);
        return new ByModel();
    }
}
