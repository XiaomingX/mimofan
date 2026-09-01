package blinded;

/*
 * JSEF-Benchmark L4 — InterceptTaint 安全对照（BX 混淆样本）
 *
 * 安全做法：拦截器写入的属性仅作为数据，控制层使用 SimpleEvaluationContext 对
 * 固定常量表达式求值，不把不可信属性文本作为表达式解析。用于计算 TN / FP。
 *
 * CWE-917 Expression Language Injection。
 */
public class InterceptTaintBy {

    public void preHandle(Object request) {
        setAttribute(request, "expr", getHeader(request, "X-Expr"));
    }

    public void handle(Object request) {
        String data = (String) getAttribute(request, "expr"); // 仅作数据
        /*ANCHOR_1*/
        parseExpression("'constant-label'"); // 不可信 data 不进入表达式文本
        System.out.println("[spel-data] " + data);
    }

    private static Object parseExpression(String expr) {
        System.out.println("[spel-eval-by] " + expr);
        return null;
    }

    private static void setAttribute(Object req, String k, String v) { /* framework semantics */ }
    private static String getAttribute(Object req, String k) { return "localhost-demo"; }
    private static String getHeader(Object req, String k) { return "localhost-demo"; }

    public static void main(String[] args) {
        InterceptTaintBy t = new InterceptTaintBy();
        Object req = new Object();
        t.preHandle(req);
        t.handle(req);
    }
}
