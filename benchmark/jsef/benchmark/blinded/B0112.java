package blinded;













public class GmDenyListBlockedBy {

    
    private static final java.util.Set<String> ALLOWLIST = java.util.Set.of(
            "com.jsef.benchmark.bx.gadgetmine.TrustedModel"
    );

    


    public static Object byParse(String typeName) {

        if (isDenyListed(typeName)) {
            /*ANCHOR_1*/
            throw new IllegalArgumentException("fastjson deny list blocked: " + typeName);  // deny 拦截
        }
        // ② allowlist 收紧可实例化范围
        if (!ALLOWLIST.contains(typeName)) {
            throw new IllegalArgumentException("type not in allowlist: " + typeName);
        }
        return byInstantiate(typeName);
    }

    private static Object byInstantiate(String typeName) {
        System.out.println("[demo-only] by-instantiating allowed type: " + typeName);
        return new Object();
    }

    
    private static boolean isDenyListed(String typeName) {
        return typeName != null
                && (typeName.startsWith("org.apache.commons.collections.functors.")
                    || typeName.startsWith("com.sun.")
                    || typeName.startsWith("org.apache.xalan."));
    }
}
