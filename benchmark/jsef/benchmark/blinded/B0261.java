
package blinded;

import java.lang.reflect.Method;
import java.nio.file.Files;
import java.nio.file.Paths;
import java.util.Arrays;
import java.util.List;










public class SBM1_BinderTraversal_By {

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

    // 黑名单：拦截 class / module / classLoader 等危险穿越前缀
    private static final List<String> DISALLOWED_PREFIXES =
            Arrays.asList("class", "module", "classLoader");

    private static boolean isDisallowed(String path) {
        for (String prefix : DISALLOWED_PREFIXES) {
            if (path.startsWith(prefix + ".")
                    || path.contains("." + prefix + ".")
                    || path.equals(prefix)) {
                return true;
            }
        }
        return false;
    }

    


    public static void bind(Object root, String path, String value)
            throws Exception {
        String[] parts = path.split("\\.");
        Object current = root;
        for (int i = 0; i < parts.length - 1; i++) {
            Method getter = current.getClass().getMethod("get" + capitalize(parts[i]));
            current = getter.invoke(current);
        }
        /*ANCHOR_1*/
        if (isDisallowed(path)) {
            throw new SecurityException("rejected binder traversal to internal object: " + path);
        }
        String last = parts[parts.length - 1];
        Method setter = current.getClass().getMethod("set" + capitalize(last), String.class);
        setter.invoke(current, value);
    }

    


    public static void bindChain(Object root, String path, String value)
            throws Exception {
        bind(root, path, value);
        Root r = (Root) root;
        String target = r.getMiddle().getInner().getLogPath();
        /*ANCHOR_2*/
        if (isDisallowed(path)) {
            // 路径被拒绝，内部字段未污染，不执行任何文件写
            return;
        }
        Files.write(Paths.get("localhost-demo/" + target), "localhost-demo".getBytes());
    }

    private static String capitalize(String s) {
        return s.substring(0, 1).toUpperCase() + s.substring(1);
    }

    // 演示入口（仅 localhost 语义，不连真实远端）
    public static void main(String[] args) throws Exception {
        Root root = new Root();
        try {
            bind(root, "middle.inner.logPath", "localhost-demo/pwned.log");
        } catch (SecurityException e) {
            // 期望：穿越被拒绝
        }
        try {
            bindChain(root, "middle.inner.logPath", "localhost-demo/pwned.log");
        } catch (SecurityException e) {
            // 期望：穿越被拒绝
        }
    }
}
