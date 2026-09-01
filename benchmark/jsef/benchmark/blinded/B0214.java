
package blinded;










public class MvcBinderStateMachine_By {

    
    private boolean allowFieldBinding = false; // 默认安全

    
    private static final String[] ALLOWED_PREFIXES = {"by.", "name.", "email."};

    static class ExpressionParser {
        static Object parseExpression(String expression) {
            return expression;
        }
    }

    private String mapParamToObjectPath(String paramName) {
        return paramName;
    }

    public Object bindAndEvaluate(String paramName, String paramValue) {
        if (allowFieldBinding) {
            String objectPath = mapParamToObjectPath(paramName);
            return ExpressionParser.parseExpression(objectPath + "=" + paramValue);
        }
        // 安全分支：先过 allowlist
        boolean allowed = false;
        for (String prefix : ALLOWED_PREFIXES) {
            if (paramName.startsWith(prefix)) {
                allowed = true;
                break;
            }
        }
        /*ANCHOR_1*/
        if (!allowed) {
            return null; // 危险路径被拒，无法到达 sink
        }
        String objectPath = mapParamToObjectPath(paramName);
        return ExpressionParser.parseExpression(objectPath + "=" + paramValue);
    }
}
