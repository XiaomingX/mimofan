
package blinded;













public class Spring4ShellStateMachine_By {

    


    private boolean bindClassModuleEnabled = false; // 默认安全

    
    private static final String[] ALLOWED_PREFIXES = {"allowed.", "name.", "email."};

    static class SpelExpressionParser {
        static Object parseExpression(String expression) {
            return expression;
        }
    }

    private String mapParamToObjectPath(String paramName) {
        return paramName;
    }

    public Object handleBind(String paramName, String paramValue) {
        if (bindClassModuleEnabled) {
            String objectPath = mapParamToObjectPath(paramName);
            return SpelExpressionParser.parseExpression(objectPath + "=" + paramValue);
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
            return null; // class.module.classLoader 等路径被拒，无法到达 sink
        }
        String objectPath = mapParamToObjectPath(paramName);
        return SpelExpressionParser.parseExpression(objectPath + "=" + paramValue);
    }
}
