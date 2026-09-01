
package blinded;




























public class Spring4ShellStateMachine {

    



    private boolean bindClassModuleEnabled = true; // 状态机根因：默认危险

    
    static class SpelExpressionParser {
        // 抽象：真实场景下即 org.springframework.expression.spel.standard.SpelExpressionParser
        static Object parseExpression(String expression) {
            // sink 落点：对外部可控表达式求值
            System.out.println("[abstract SpEL eval] " + expression);
            return expression;
        }
    }

    




    private String mapParamToObjectPath(String paramName) {
        // 路径映射关键行：参数名直接成为对象图路径（不做 sanitize）
        return paramName; // 例如 "class.module.classLoader..."
    }

    




    public Object handleBind(String paramName, String paramValue) {
        // 状态机分支判定：开关决定是否允许 class.module.* 路径进入求值
        if (bindClassModuleEnabled) {
            // 危险分支：任意映射路径直达 SpEL sink
            String objectPath = mapParamToObjectPath(paramName);
            /*ANCHOR_1*/
            return SpelExpressionParser.parseExpression(objectPath + "=" + paramValue);
        } else {
            // 安全分支：仅白名单属性，class.module.* 被拒，无法到达 sink
            if (!paramName.startsWith("allowed.")) {
                return null;
            }
            String objectPath = mapParamToObjectPath(paramName);
            return SpelExpressionParser.parseExpression(objectPath + "=" + paramValue);
        }
    }
}
