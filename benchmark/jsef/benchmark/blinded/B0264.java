package blinded;










public class SBM4_AuthzBypass_By {

    public static class Request {
        public String callerId; // 已认证身份（服务端设置，非用户可控）
        public String action;   // 请求的动作
    }

    public enum AuthorizationDecision {
        ALLOW, DENY
    }

    


    private static boolean permitted(String callerId, String action) {
        // localhost-demo：仅占位矩阵
        return "svc-admin".equals(callerId) && "config.reload".equals(action);
    }

    


    
    public static AuthorizationDecision isAuthorized(Request req) {
        AuthorizationDecision decision;
        if (req.callerId != null && permitted(req.callerId, req.action)) {
            decision = AuthorizationDecision.ALLOW;
        } else {
            decision = AuthorizationDecision.DENY;
        }
        /*ANCHOR_1*/
        return decision;
    }

    


    
    public static AuthorizationDecision decide(Request req) {
        AuthorizationDecision decision;
        if (req.callerId != null && permitted(req.callerId, req.action)) {
            decision = AuthorizationDecision.ALLOW;
        } else {
            decision = AuthorizationDecision.DENY;
        }
        /*ANCHOR_2*/
        return decision;
    }
}
