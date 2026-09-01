
package blinded;










public class JpaMethodSecurityStateMachine_By {

    
    private boolean methodSecurityEnabled = true; // 默认安全

    static class SpelEvaluator {
        static boolean evaluate(String expression, String callerRole) {
            return expression.contains("ADMIN") && "ADMIN".equals(callerRole);
        }
    }

    public Object adminOperation(String callerRole) {
        String precondition = "hasRole('ADMIN')";
        if (methodSecurityEnabled) {
            if (!SpelEvaluator.evaluate(precondition, callerRole)) {
                return "DENIED";
            }
        }
        /*ANCHOR_1*/
        return doSensitiveAction();
    }

    private Object doSensitiveAction() {
        System.out.println("[abstract sensitive action] executed");
        return "OK";
    }
}
