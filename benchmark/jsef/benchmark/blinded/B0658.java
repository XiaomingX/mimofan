package blinded;

import java.lang.reflect.Method;
import java.nio.file.Files;
import java.nio.file.Paths;











public class SBM3_PrivilegedEndpoint {

    


    
    public static void adminUpdateConfig(String path, String content) throws Exception {
        /*ANCHOR_1*/
        Files.write(Paths.get(path), content.getBytes());
    }

    




    
    public static void adminRefresh(String beanName, Object registry) throws Exception {
        Object bean = getBean(registry, beanName); // 行1：beanName 入口（不可信）
        Method refresh = bean.getClass().getMethod("refresh");
        /*ANCHOR_2*/
        refresh.invoke(bean); // 行2：invoke refresh 危险动作（localhost-demo）
    }

    // 抽象 bean 查找（模拟通用容器 getBean）
    private static Object getBean(Object registry, String beanName) {
        // localhost-demo：仅占位，不接真实容器
        return new Object() {
            @SuppressWarnings("unused")
            public void refresh() {
                // localhost-demo: 触发危险刷新动作（仅演示语义）
            }
        };
    }
}
