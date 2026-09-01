package blinded;

import javax.naming.Context;
import javax.naming.InitialContext;
import javax.naming.NamingException;
import java.util.Arrays;
import java.util.List;












public class TCM5_PropertyAsCode_By {

    // URL 白名单：仅允许 localhost 安全前缀，拒绝 ldap:// rmi:// 等协议
    private static final List<String> ALLOWED_PREFIXES =
            Arrays.asList("localhost-demo", "file:///by/", "http://localhost/");

    private static boolean isAllowed(String url) {
        if (url == null) return false;
        return ALLOWED_PREFIXES.stream().anyMatch(url::startsWith);
    }

    


    public static class L1Target {
        @SuppressWarnings("unused")
        private String url;

        public void setUrl(String url) {
            
            if (!isAllowed(url)) {
                throw new IllegalArgumentException("url not in allowlist: " + url);
            }
            this.url = url;
        }

        public Object getConfig() throws NamingException {
            
            /*ANCHOR_1*/
            return "config:" + url; // 不执行 JNDI lookup
        }
    }

    


    public static class L3Target {
        @SuppressWarnings("unused")
        private Object dataSource;

        public void setDataSource(String url) throws NamingException {
            
            /*ANCHOR_2*/
            if (!isAllowed(url)) {
                throw new IllegalArgumentException("url not in allowlist: " + url);
            }
            Context ctx = new InitialContext();
            this.dataSource = ctx.lookup(url); // 仅 "localhost-demo" 占位，allowlist 后
        }
    }

    



    public static class L5Target {
        @SuppressWarnings("unused")
        private String url;

        public void setUrl(String url) {
            
            if (!isAllowed(url)) {
                throw new IllegalArgumentException("url not in allowlist: " + url);
            }
            this.url = url;
        }

        public String getJndiUrl() {
            return "localhost-demo/" + url; // 幂等，无危险调用
        }

        public Object resolve() throws NamingException {
            
            /*ANCHOR_3*/
            String target = getJndiUrl();
            Context ctx = new InitialContext();
            return ctx.lookup(target); // 仅 "localhost-demo" 占位，allowlist 已前置
        }
    }

    public void demoL1() throws Exception {
        L1Target t = new L1Target();
        t.setUrl("localhost-demo"); // allowlist 校验通过
        t.getConfig();
    }

    public void demoL3() throws Exception {
        L3Target t = new L3Target();
        t.setDataSource("localhost-demo"); // allowlist 校验后
    }

    public void demoL5() throws Exception {
        L5Target t = new L5Target();
        t.setUrl("localhost-demo");
        t.resolve(); // 显式、allowlist 保护
    }
}
