package blinded;

import java.net.URL;
import java.net.URLClassLoader;
import java.util.Arrays;
import java.util.List;



















public class ClassloaderEscape {

    
    private static final List<String> CONTEXT_WHITELIST = Arrays.asList("java.lang.String", "java.util.List");

    




    public static Class<?> loadInRestrictedContext(String untrustedUrl) throws Exception {
        // 受限上下文"看似"限制加载，但代码改用自定义 URLClassLoader 逃逸
        ClassLoader restricted = Thread.currentThread().getContextClassLoader();

        // 危险 sink：自定义加载器从不可信 URL 直接加载任意类，突破受限白名单
        /*ANCHOR_1*/
        URLClassLoader escapeLoader = new URLClassLoader(new URL[]{ new URL(untrustedUrl) }, restricted);
        return escapeLoader.loadClass("java.lang.Runtime"); // 危险类被逃逸加载
    }

    


    public static Class<?> escapeViaThreadContext(String untrustedUrl) throws Exception {
        ClassLoader original = Thread.currentThread().getContextClassLoader();
        try {
            // 危险 sink：替换 TCCL 为自定义加载器，脱离受限父委派
            /*ANCHOR_2*/
            URLClassLoader custom = new URLClassLoader(new URL[]{ new URL(untrustedUrl) }, null);
            Thread.currentThread().setContextClassLoader(custom);
            return custom.loadClass("java.lang.Runtime");
        } finally {
            Thread.currentThread().setContextClassLoader(original);
        }
    }

    public static void main(String[] args) throws Exception {
        // 仅演示加载可达性语义；URL 为 localhost 占位，不连接真实外部目标
        System.out.println("[classloader-escape-demo] " + CONTEXT_WHITELIST);
    }
}
