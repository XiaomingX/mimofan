package blinded;

import java.lang.reflect.Method;















public class ConfusionDeserWhitelistType {

    // 看似"白名单"，但名单内的类型本身即可执行危险操作
    static final java.util.Set<String> TRUSTED = java.util.Set.of(
            "com.demo.LegacyHandler", "com.demo.LegacyService");

    


    static Object unbyDeser(String className, String methodName) throws Exception {
        if (!TRUSTED.contains(className)) {
            throw new IllegalArgumentException("type not allowed"); // 仅校验类名
        }
        Class<?> clazz = Class.forName(className); // 受名单约束，但类本身危险
        Object instance = clazz.getDeclaredConstructor().newInstance();
        Method m = clazz.getMethod(methodName); // 方法名来自输入
        /*ANCHOR_1*/
        return m.invoke(instance); // 反射调用危险方法：仍不安全反序列化利用链
    }

    



    static String byDeser(String className) {
        // 真正的白名单：只允许一个无副作用的只读方法
        if (!"com.demo.LegacyService".equals(className)) {
            throw new IllegalArgumentException("type not allowed");
        }
        /*ANCHOR_2*/
        return "ok"; // 不反射、不执行任意方法，安全
    }
}
