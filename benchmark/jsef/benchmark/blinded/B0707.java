package blinded;

import java.lang.reflect.Method;


















public class TCM1_DirectTypeSelect {

    
    public Object handleL1(String userInput) throws Exception {
        // userInput 来自 HTTP 请求体（不可信），直接作为类名
        Class<?> c = Class.forName(userInput);
        /*ANCHOR_1*/
        Object o = c.getDeclaredConstructor().newInstance(); // 隐式调用构造器
        return o;
    }

    
    public Object handleL3(String json) throws Exception {
        // 极简 json 解析（自包含，不依赖任何库），提取 cls 与 arg 字段
        String cls = extractField(json, "cls");
        String arg = extractField(json, "arg"); // 占位参数 "localhost-demo"

        // 加载攻击者控制的类并实例化
        /*ANCHOR_2*/
        Object o = ClassLoader.getSystemClassLoader().loadClass(cls).getDeclaredConstructor().newInstance();

        // 反射调用隐式危险方法 init()（演示 init 内部可达 sink，这里仅占位打印+字符串）
        Method init = o.getClass().getDeclaredMethod("init", String.class);
        init.invoke(o, arg);
        return o;
    }

    // 极简字段提取，仅用于演示，不要求健壮性
    private static String extractField(String json, String key) {
        int i = json.indexOf("\"" + key + "\"");
        if (i < 0) return "";
        int colon = json.indexOf(':', i);
        int q1 = json.indexOf('"', colon);
        int q2 = json.indexOf('"', q1 + 1);
        return json.substring(q1 + 1, q2);
    }
}
