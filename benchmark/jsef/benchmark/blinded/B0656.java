
package blinded;

import java.lang.reflect.Method;
import java.nio.file.Files;
import java.nio.file.Paths;
















public class SBM1_BinderTraversal {

    


    public static class Inner {
        private String logPath = "localhost-demo/app.log";
        public String getLogPath() { return logPath; }
        public void setLogPath(String logPath) { this.logPath = logPath; }
    }

    public static class Middle {
        private Inner inner = new Inner();
        public Inner getInner() { return inner; }
        public void setInner(Inner inner) { this.inner = inner; }
    }

    public static class Root {
        private Middle middle = new Middle();
        public Middle getMiddle() { return middle; }
        public void setMiddle(Middle middle) { this.middle = middle; }
    }

    




    public static void bind(Object root, String path, String value)
            throws Exception {
        String[] parts = path.split("\\.");
        Object current = root;
        for (int i = 0; i < parts.length - 1; i++) { // path 解析穿越起点
            Method getter = current.getClass().getMethod("get" + capitalize(parts[i]));
            current = getter.invoke(current);
        }
        String last = parts[parts.length - 1];
        Method setter = current.getClass().getMethod("set" + capitalize(last), String.class);
        /*ANCHOR_1*/
        setter.invoke(current, value);
    }

    



    public static void bindChain(Object root, String path, String value)
            throws Exception {
        // 先穿越绑定，污染内部危险属性
        bind(root, path, value);
        // 取回内部对象的危险字段，作为文件写路径
        Root r = (Root) root;
        String target = r.getMiddle().getInner().getLogPath();
        /*ANCHOR_2*/
        Files.write(Paths.get("localhost-demo/" + target), "localhost-demo".getBytes());
    }

    private static String capitalize(String s) {
        return s.substring(0, 1).toUpperCase() + s.substring(1);
    }

    // 演示入口（仅 localhost 语义，不连真实远端）
    public static void main(String[] args) throws Exception {
        Root root = new Root();
        // 类比 class.module.classLoader 穿越：middle.inner.logPath
        bind(root, "middle.inner.logPath", "localhost-demo/pwned.log");
        bindChain(root, "middle.inner.logPath", "localhost-demo/pwned.log");
    }
}
