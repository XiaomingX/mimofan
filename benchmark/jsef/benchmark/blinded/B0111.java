package blinded;

import javax.naming.InitialContext;
import javax.naming.NamingException;



















public class GmCacheBypassBy {

    



    public static void byDemo(String typeName, String dataSourceName) {
        /*ANCHOR_1*/
        if ("java.lang.Class".equals(typeName)) { // 安全处理行：拒绝缓存写路径，阻断 cache bypass
            throw new IllegalArgumentException("java.lang.Class cache path denied (patched in 1.2.68+)");
        }
        // 即便通过，亦不触发 lookup：安全实现中 setter 不再驱动 JNDI sink
        ByRowSet stub = new ByRowSet();
        stub.setDataSourceNameBy(dataSourceName);
    }

    


    public static class ByRowSet {
        private String dataSourceName;
        public void setDataSourceNameBy(String dataSourceName) {
            this.dataSourceName = dataSourceName; // 安全：仅本地字段，无 sink 调用
        }
    }
}
