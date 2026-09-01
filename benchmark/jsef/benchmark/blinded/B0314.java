package blinded;

import java.lang.reflect.Method;













public class TCM1_DirectTypeSelect_By {

    // 服务端固定白名单：键由用户传，值为服务端预置、用户不可控的类
    private static final java.util.Map<String, Class<?>> ALLOWED = java.util.Map.of(
            "demo", DemoBean.class
    );

    
    public Object handleL1(String userKey) throws Exception {
        /*ANCHOR_1*/
        Class<?> c = ALLOWED.get(userKey);
        if (c == null) {
            throw new IllegalArgumentException("unknown key");
        }
        Object o = c.getDeclaredConstructor().newInstance();
        return o;
    }

    
    public Object handleL3(String json) throws Exception {
        String key = extractField(json, "key");
        String arg = extractField(json, "arg"); // 占位参数 "localhost-demo"

        /*ANCHOR_2*/
        Class<?> c = ALLOWED.get(key);
        if (c == null) {
            throw new IllegalArgumentException("unknown key");
        }
        Object o = c.getDeclaredConstructor().newInstance();

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

    // 服务端白名单内的安全类：init() 仅占位，不含危险 sink
    public static class DemoBean {
        public void init(String arg) {
            // 占位：仅打印，不执行任何危险操作
            System.out.println("DemoBean.init with arg=" + arg);
        }
    }
}
