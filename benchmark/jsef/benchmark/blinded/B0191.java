package blinded;















public class FastjsonCrossFile_By {

    
    private static final java.util.Set<String> ALLOWLIST = java.util.Set.of(
            "com.example.LocalModel",
            "com.example.ByDto"
    );

    


    public static Object byProcess(String untrustedTypeName) {
        /*ANCHOR_1*/
        if (!ALLOWLIST.contains(untrustedTypeName)) {   // 安全处理行：allowlist 校验
            throw new IllegalArgumentException("type not allowed: " + untrustedTypeName);
        }
        return byInstantiate(untrustedTypeName);
    }

    


    private static Object byInstantiate(String typeName) {
        System.out.println("[demo-only] by-instantiating allowed type: " + typeName);
        return new Object();
    }
}
