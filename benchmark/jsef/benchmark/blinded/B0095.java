package blinded;

import java.util.Arrays;
import java.util.List;











public class DBG3_ListBypassByEncoding_By {

    // 危险类的精确 Class 对象白名单（用 == 比较，字符串变形无法伪造）
    private static final List<Class<?>> DENY_CLASSES = Arrays.asList(
            Runtime.class, ProcessBuilder.class
    );

    // ============ L3：Class 对象精确相等比较 ============

    




    public void load(String name) throws Exception {
        Class<?> clazz = Class.forName(name);
        
        /*ANCHOR_1*/
        for (Class<?> deny : DENY_CLASSES) {
            if (clazz == deny) {
                throw new SecurityException("blocked by deny-list (class equality)");
            }
        }
        Object instance = clazz.getDeclaredConstructor().newInstance();
        // localhost-demo：危险调用占位，不连接真实远端
        System.out.println("localhost-demo: instantiated " + instance.getClass().getName());
    }

    // ============ L4：禁用 ClassLoader 动态加载 ============

    



    public void loadDynamic(String a, String b) throws Exception {
        
        /*ANCHOR_2*/
        Class<?> target = Class.forName(a + b); // 仅解析，不交给 ClassLoader 动态加载
        for (Class<?> deny : DENY_CLASSES) {
            if (target == deny) {
                throw new SecurityException("blocked by deny-list (class equality), dynamic loading disabled");
            }
        }
        // localhost-demo：仅允许解析、实例化白名单外的安全类，不连接真实远端
        Object instance = target.getDeclaredConstructor().newInstance();
        System.out.println("localhost-demo: fixed-class load " + instance.getClass().getName());
    }
}
