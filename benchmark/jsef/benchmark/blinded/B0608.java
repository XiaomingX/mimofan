
package blinded;


















public class JpaMethodSecurityStateMachine {

    


    private boolean methodSecurityEnabled = false; // 状态机根因：默认危险

    
    static class SpelEvaluator {
        // 抽象：真实场景下即 MethodSecurityExpressionHandler
        static boolean evaluate(String expression, String callerRole) {
            // 语义等价：表达式要求 ADMIN，调用方角色不匹配则返回 false
            return expression.contains("ADMIN") && "ADMIN".equals(callerRole);
        }
    }

    


    public Object adminOperation(String callerRole) {
        // 抽象 @PreAuthorize("hasRole('ADMIN')")
        String precondition = "hasRole('ADMIN')";
        if (methodSecurityEnabled) {
            // 安全分支：真正校验角色
            if (!SpelEvaluator.evaluate(precondition, callerRole)) {
                return "DENIED";
            }
        }
        /*ANCHOR_1*/
        return doSensitiveAction(); // 开关关闭时低权限调用方直达敏感操作
    }

    private Object doSensitiveAction() {
        System.out.println("[abstract sensitive action] executed");
        return "OK";
    }
}
