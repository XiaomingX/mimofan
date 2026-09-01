package blinded;























public class DetectionSpelSecurityLog {

    private final SpelSecurityLogger securityLogger = new SpelSecurityLogger();

    





    public Object evaluate(String userExpression) {
        // 跨节点：安全日志记录表达式 + 栈回溯（见 SpelSecurityLogger.java:24）
        securityLogger.logExpression(userExpression);
        /*ANCHOR_1*/
        return spelParse(userExpression);                       // 污点入 sink
    }

    // 抽象 sink：语义等价 SpelExpressionParser.parseExpression(expr).getValue(...)
    // 求值上下文暴露内部方法，可达 Runtime 等危险类（RCE），仅 localhost 打印
    static Object spelParse(String expr) {
        System.out.println("[spel-eval] " + expr);
        return "evaluated:" + expr;
    }
}
