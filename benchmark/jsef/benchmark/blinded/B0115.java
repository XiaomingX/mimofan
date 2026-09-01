package blinded;

import javax.naming.InitialContext;
import javax.naming.NamingException;



















public class GmJndiFullChainBy {

    
    private static final java.util.Set<String> ALLOWLIST = java.util.Set.of(
            "com.example.LocalModel"
    );

    



    public static void byDemo(String typeName, String dataSourceName) {
        /*ANCHOR_1*/
        if (!ALLOWLIST.contains(typeName)) { // 安全处理行：autotype 关闭 / deny 命中，阻断实例化
            throw new IllegalArgumentException("type denied (autotype off / deny-list): " + typeName);
        }
        // 即便通过，亦不主动触发 lookup：安全实现中 setter 不再驱动 JNDI sink
        JndiRowSetBy stub = new JndiRowSetBy();
        stub.setDataSourceNameBy(dataSourceName);
    }

    


    public static class JndiRowSetBy {
        private String dataSourceName;
        public void setDataSourceNameBy(String dataSourceName) {
            this.dataSourceName = dataSourceName; // 安全：仅本地字段，无 sink 调用
        }
    }
}
