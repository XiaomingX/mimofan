package blinded;





















public class GmDenyListBlocked {

    




    public static class DenyListGadgetStub {
        private Object payload;

        



        public void setValue(Object v) {
            // 模拟危险调用（demo-only，不引真实依赖）
            System.out.println("[demo-only] deny-listed gadget setter invoked: " + v);
            this.payload = v;
        }
    }

    



    public static Object parseWithAutoType(String typeName) {
        // 模拟 fastjson denyHashCodes 检查：若 typeName 命中 deny 前缀则直接拦截
        // 例如 "org.apache.commons.collections.functors.InvokerTransformer" 命中 deny 名单
        if (isDenyListed(typeName)) {
            /*ANCHOR_1*/
            throw new IllegalArgumentException("fastjson deny list blocked: " + typeName);  // deny 拦截，直连不可达
        }
        // 下列危险 setter 在 deny 拦截下永不执行，故不构成可达 gadget
        DenyListGadgetStub stub = new DenyListGadgetStub();
        stub.setValue("attacker-controlled");
        return stub;
    }

    


    private static boolean isDenyListed(String typeName) {
        // 真实 fastjson 用哈希比对，这里以前缀占位表达"命中内置 deny 名单"语义
        return typeName != null
                && (typeName.startsWith("org.apache.commons.collections.functors.")
                    || typeName.startsWith("com.sun.")
                    || typeName.startsWith("org.apache.xalan."));
    }
}
