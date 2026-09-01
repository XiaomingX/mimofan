package blinded;

























public class FastjsonCrossFilePerturbed_By {

    
    private static final java.util.Set<String> ALLOWLIST = java.util.Set.of(
            "com.example.LocalModel",
            "com.example.ByDto"
    );

    


    public static Object byProcess(String rawPayload) {
        /*ANCHOR_1*/
        if (!ALLOWLIST.contains(rawPayload)) {   // 安全处理行：allowlist 校验
            throw new IllegalArgumentException("type not allowed: " + rawPayload);
        }
        return secureBuild(rawPayload);
    }

    


    private static Object secureBuild(String clz) {
        System.out.println("[demo-only] by-instantiating allowed type: " + clz);
        return new Object();
    }
}
