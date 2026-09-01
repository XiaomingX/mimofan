package blinded;














public class DBG3_ListBypassByEncoding {

    // 危险类名的字符串黑名单（仅用于演示字符串匹配型防护的脆弱性）
    private static final String[] DENY_LIST = {"runtime", "processbuilder", "exec"};

    // ============ L3：单方法内编码/变形绕过黑名单 ============

    





    public void load(String name) throws Exception {
        // 行1：黑名单检查被绕过点 —— 攻击者传入变形类名使字符串匹配失败
        boolean blocked = false;
        for (String deny : DENY_LIST) {
            if (name.toLowerCase().contains(deny)) {
                blocked = true;
                break;
            }
        }
        if (blocked) {
            throw new SecurityException("blocked by deny-list");
        }
        
        /*ANCHOR_1*/
        String resolved = name.replace(".", ""); // 去掉点分隔变形
        Class<?> clazz = Class.forName(resolved);
        Object instance = clazz.newInstance();
        // localhost-demo：危险调用占位，不连接真实远端
        System.out.println("localhost-demo: instantiated " + instance.getClass().getName());
    }

    // ============ L4：字符串拼接 / 反射拼名跨节点绕过 ============

    





    public void loadDynamic(String a, String b) throws Exception {
        // 行1：拼接点 —— 攻击者将危险类名拆成两段拼回，绕过精确字符串匹配
        String resolved = a + b;
        
        /*ANCHOR_2*/
        ClassLoader cl = getClass().getClassLoader();
        Class<?> clazz = cl.loadClass(resolved);
        Object instance = clazz.getDeclaredConstructor().newInstance();
        // localhost-demo：危险调用占位，不连接真实远端
        System.out.println("localhost-demo: dynamically loaded " + instance.getClass().getName());
    }
}
