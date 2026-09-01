package blinded;











public class DBG3_EncodingVariants {

    // 危险类名的平面字符串黑名单（演示字符串匹配型防护的脆弱性）
    private static final String[] DENY_LIST = {"runtime", "processbuilder"};

    // ============ L3：嵌套包装变体 ============

    




    public void loadNested(String name) throws Exception {
        // 行1：黑名单检查被绕过点 —— 嵌套包装使平面字符串匹配失效
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
        Class<?> clazz = Class.forName(name);
        Object instance = clazz.newInstance();
        // localhost-demo：危险调用占位，不连接真实远端
        System.out.println("localhost-demo: nested instantiated " + instance.getClass().getName());
    }

    // ============ L4：转义/双写变体 ============

    




    public void loadEscaped(String obfuscated) throws Exception {
        // 行1：转义/双写还原点 —— 攻击者插入的不可见字符被删去，危险类名被拼回
        String resolved = obfuscated.replace("\u200b", "").replace("timetime", "time");
        
        /*ANCHOR_2*/
        ClassLoader cl = getClass().getClassLoader();
        Class<?> clazz = cl.loadClass(resolved);
        Object instance = clazz.getDeclaredConstructor().newInstance();
        // localhost-demo：危险调用占位，不连接真实远端
        System.out.println("localhost-demo: escaped load " + instance.getClass().getName());
    }
}
