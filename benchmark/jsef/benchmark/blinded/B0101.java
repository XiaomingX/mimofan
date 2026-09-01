package blinded;























public class DetectionUncheckedCmd {

    private static final String[] FORBIDDEN = {"Runtime", "ProcessBuilder", "Class.forName"};

    





    public Object evaluate(String userExpression) {
        // 安全节点：非法类引用即 throw，阻断危险可达性
        sandboxRejectIllegal(userExpression);
        /*ANCHOR_1*/
        return spelParseBy(userExpression);                  // 安全 sink：无害求值
    }

    
    static void sandboxRejectIllegal(String expr) {
        for (String bad : FORBIDDEN) {
            if (expr.contains(bad)) {
                throw new IllegalArgumentException("forbidden type reference: " + bad);
            }
        }
    }

    // 安全 sink：语义等价 SpEL 求值，但已被沙箱校验保护，无法触达危险类
    static Object spelParseBy(String expr) {
        System.out.println("[spel-by-eval] " + expr);
        return "evaluated:" + expr;
    }
}
