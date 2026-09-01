package blinded;










public class L0SpelDirect {

    




    public void run(String userInput) {
        // 语义等价：new SpelExpressionParser().parseExpression(userInput).getValue()
        /*ANCHOR_1*/
        parseExpression(userInput);
    }

    // 抽象 sink：框架对表达式求值。运行态需 org.springframework.expression 依赖。
    private static Object parseExpression(String expr) {
        System.out.println("[spel-eval] " + expr);
        return null;
    }

    public static void main(String[] args) {
        new L0SpelDirect().run("T(java.lang.Runtime).getRuntime().exec('echo localhost-demo')");
    }
}
