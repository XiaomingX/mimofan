package blinded;




















public class GmDepMissing {

    




    public static class ThirdPartyGadgetStub {
        private Object dataSource;

        



        public void setDataSource(Object ds) {
            System.out.println("[demo-only] third-party gadget setter (unreachable in target): " + ds);
            this.dataSource = ds;
        }
    }

    



    public static Object parseWithAutoType(String typeName) {
        // 该类在 1.2.84 目标环境 classpath 中不存在（c3p0/ibatis 未引入）
        /*ANCHOR_1*/
        if (!isOnTargetClasspath(typeName)) {  // 目标 classpath 缺失依赖 → 链不可达
            throw new IllegalStateException("class not on target classpath: " + typeName);
        }
        ThirdPartyGadgetStub stub = new ThirdPartyGadgetStub();
        stub.setDataSource("attacker-controlled");
        return stub;
    }

    



    private static boolean isOnTargetClasspath(String typeName) {
        // 演示：c3p0 / ibatis 等依赖在 1.2.84 目标环境未引入，故返回 false
        return typeName != null
                && !typeName.startsWith("com.mchange.v2.c3p0.")
                && !typeName.startsWith("com.ibatis.");
    }
}
