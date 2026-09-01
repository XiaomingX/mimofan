package blinded;

import javax.naming.Context;
import javax.naming.InitialContext;
import javax.naming.NamingException;
import java.net.InetAddress;
import java.net.UnknownHostException;

















public class TCM5_PropertyAsCode {

    



    public static class L1Target {
        @SuppressWarnings("unused")
        private String url; // 由 setter 接收不可信输入

        public void setUrl(String url) { // setter：接收不可信输入
            this.url = url;
        }

        public Object getConfig() throws NamingException, UnknownHostException {
            
            Context ctx = new InitialContext();
            /*ANCHOR_1*/
            return ctx.lookup(url); // 仅占位 "localhost-demo"，不连真实远端
        }
    }

    



    public static class L3Target {
        @SuppressWarnings("unused")
        private Object dataSource;

        public void setDataSource(String url) throws NamingException { // 行：setter 入口
            Context ctx = new InitialContext();
            
            /*ANCHOR_2*/
            dataSource = ctx.lookup(url); // 行：lookup 调用，仅占位 "localhost-demo"
        }
    }

    




    public static class L5Target {
        @SuppressWarnings("unused")
        private String url; // 行：url 字段（入口，由外部设置）

        public void setUrl(String url) { // 行：入口 setter，外部不可信输入写入 url
            this.url = url;
        }

        public String getJndiUrl() { // 行：中间 getter
            return "ldap://localhost-demo/" + url; // 仅占位，不连真实远端
        }

        public Object resolve() throws NamingException {
            // getter 链：resolve -> getJndiUrl 构造目标 -> 末节点 lookup
            String target = getJndiUrl(); // 中间节点：getter 链
            Context ctx = new InitialContext();
            
            /*ANCHOR_3*/
            return ctx.lookup(target); // 行：lookup 调用，仅占位 "localhost-demo"
        }
    }

    


    public void demoL1() throws Exception {
        L1Target t = new L1Target();
        t.setUrl("ldap://localhost-demo/evil"); // 不可信输入
        t.getConfig(); // 属性访问 = 代码执行
    }

    


    public void demoL3() throws Exception {
        L3Target t = new L3Target();
        t.setDataSource("ldap://localhost-demo/evil"); // setter 体内直接 lookup
    }

    


    public void demoL5() throws Exception {
        L5Target t = new L5Target();
        t.setUrl("localhost-demo"); // 入口：不可信输入写入 url 字段
        t.resolve(); // getter 链 -> JNDI lookup
    }
}
