
package blinded;






















public class MvcBinderStateMachine {

    


    private boolean allowFieldBinding = true; // 状态机根因：默认危险

    
    static class ExpressionParser {
        // 抽象：真实场景下即 SpEL/OGNL 求值器
        static Object parseExpression(String expression) {
            // sink 落点：对外部可控表达式求值
            System.out.println("[abstract eval] " + expression);
            return expression;
        }
    }

    



    private String mapParamToObjectPath(String paramName) {
        // 路径映射关键行：参数名直接成为对象图路径（不做 sanitize）
        return paramName;
    }

    


    public Object bindAndEvaluate(String paramName, String paramValue) {
        if (!allowFieldBinding) {
            // 安全分支：仅允许白名单属性
            if (!paramName.startsWith("by.")) {
                return null;
            }
        }
        String objectPath = mapParamToObjectPath(paramName);
        /*ANCHOR_1*/
        return ExpressionParser.parseExpression(objectPath + "=" + paramValue);
    }
}
