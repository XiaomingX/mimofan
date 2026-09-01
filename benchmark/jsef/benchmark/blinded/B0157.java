package blinded;









public class L0SpelDirectBy {

    




    public void run(String userInput) {
        // 语义等价：new SpelExpressionParser().parseExpression("'localhost-demo'").getValue(SimpleEvaluationContext)
        /*ANCHOR_1*/
        parseExpression("'constant-label'"); // 常量表达式，userInput 作数据不参与求值
        System.out.println("[spel-data] " + userInput);
    }

    private static Object parseExpression(String expr) {
        System.out.println("[spel-eval-by] " + expr);
        return null;
    }

    public static void main(String[] args) {
        new L0SpelDirectBy().run("localhost-demo");
    }
}
