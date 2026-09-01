package blinded;













public class GmDepMissingBy {

    
    private static final java.util.Set<String> ALLOWLIST = java.util.Set.of(
            "com.jsef.benchmark.bx.gadgetmine.TrustedModel"
    );

    


    public static Object byParse(String typeName) {
        /*ANCHOR_1*/
        if (!ALLOWLIST.contains(typeName)) {  // 白名单收敛：依赖存在也不放行未授权类
            throw new IllegalArgumentException("type not in allowlist: " + typeName);
        }
        return byInstantiate(typeName);
    }

    private static Object byInstantiate(String typeName) {
        System.out.println("[demo-only] by-instantiating allowed type: " + typeName);
        return new Object();
    }
}
