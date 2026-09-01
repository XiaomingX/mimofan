package blinded;













public class GmExpectClassBlockedBy {

    
    private static final java.util.Set<String> ALLOWLIST = java.util.Set.of(
            "com.jsef.benchmark.bx.gadgetmine.GmExpectClassBlockedBy.ByDto"
    );

    


    public static class ByDto implements AutoCloseable {
        @Override
        public void close() {
            // 安全资源释放（占位）
        }
    }

    


    public static Object byResolveViaExpectClass(String typeName) {
        /*ANCHOR_1*/
        if (!ALLOWLIST.contains(typeName)) {   // 白名单校验：拒绝非受信类型
            throw new IllegalArgumentException("type not allowed: " + typeName);
        }
        System.out.println("[demo-only] by-resolve allowed type (1.2.68+ block active): " + typeName);
        return new ByDto();
    }
}
