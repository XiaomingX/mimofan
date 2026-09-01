package blinded;

import java.net.URL;
import java.net.URLClassLoader;
import java.util.Arrays;
import java.util.List;














public class ClassloaderEscapeBy {

    
    private static final List<String> CONTEXT_WHITELIST = Arrays.asList("java.lang.String", "java.util.List");

    




    public static Class<?> loadInRestrictedContextBy(String untrustedUrl) throws Exception {
        ClassLoader parent = Thread.currentThread().getContextClassLoader();

        // 安全策略：即便构造自定义加载器，也先走父委派白名单，外部 URL 不生效
        // 实际只从父加载器（白名单）解析，不可信 URL 被忽略
        /*ANCHOR_1*/
        if (CONTEXT_WHITELIST.contains("java.lang.String")) {
            return parent.loadClass("java.lang.String"); // 仅白名单父委派，无逃逸
        }
        throw new SecurityException("class not in restricted whitelist");
    }

    


    public static Class<?> byNoThreadContextEscape(String untrustedUrl) throws Exception {
        /*ANCHOR_2*/
        return Thread.currentThread().getContextClassLoader().loadClass("java.lang.String");
    }

    public static void main(String[] args) throws Exception {
        loadInRestrictedContextBy("http://localhost-demo/");
    }
}
