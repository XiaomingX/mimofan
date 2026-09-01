package blinded;


















public class GmNoDefaultCtor {

    



    // 占位类：仅带参私有构造器，无 public 无参构造、无 @JSONCreator。
    public static class NoDefaultCtorGadgetStub {
        // 仅私有带参构造，fastjson 无法实例化此类
        private NoDefaultCtorGadgetStub(String secret, int nonce) {
            // 私有带参构造：AutoType 无入口可实例化
        }
    }

    



    public static Object resolveViaType(String typeName) {
        // @type 指定 NoDefaultCtorGadgetStub
        /*ANCHOR_1*/
        Object instance = instantiateByType(typeName);
        return instance;
    }

    



    private static Object instantiateByType(String typeName) {
        System.out.println("[demo-only] attempt instantiate (no default ctor): " + typeName);
        return null;  // 占位：无默认构造 -> 实例化失败
    }
}
