package blinded;

import java.lang.reflect.Method;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;










public class SBM3_PrivilegedEndpoint_By {

    // 白名单目录：仅允许写在此目录下
    private static final Path ALLOWED_DIR = Paths.get("localhost-demo", "config");

    


    
    public static void adminUpdateConfig(String callerRole, String path, String content) throws Exception {
        checkAuthorized(callerRole); // 鉴权
        Path target = Paths.get(path).normalize();
        if (!target.startsWith(ALLOWED_DIR)) {
            throw new IllegalStateException("path not in allowlist");
        }
        /*ANCHOR_1*/
        Files.write(target, content.getBytes());
    }

    


    
    public static void adminRefresh(String callerRole, String beanName, Object registry) throws Exception {
        checkAuthorized(callerRole);
        /*ANCHOR_2*/
        Object bean = getBean(registry, beanName);
        Method refresh = bean.getClass().getMethod("refresh");
        refresh.invoke(bean); // localhost-demo
    }

    private static void checkAuthorized(String callerRole) {
        if (callerRole == null || !callerRole.equals("admin")) {
            throw new IllegalStateException("unauthorized");
        }
    }

    private static Object getBean(Object registry, String beanName) {
        // localhost-demo：仅占位
        return new Object() {
            @SuppressWarnings("unused")
            public void refresh() {
                // localhost-demo
            }
        };
    }
}
