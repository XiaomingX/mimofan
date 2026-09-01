package blinded;










public class SBM4_AuthzBypass {

    
    public static class Request {
        public String role;
        public String override;     // 用户可控的越权开关
        public String headerSecret; // 可伪造的内部密钥头
        public String url;          // 可伪造的请求 URL
    }

    
    public enum AuthorizationDecision {
        ALLOW, DENY
    }

    


    
    public static AuthorizationDecision isAuthorized(Request req) {
        AuthorizationDecision decision = AuthorizationDecision.DENY;
        if (req.role != null && req.role.equals("admin")) {
            decision = AuthorizationDecision.ALLOW;
        } else if (req.override != null) {
            // 攻击者控制 override 即绕过
            decision = AuthorizationDecision.ALLOW;
        }
        /*ANCHOR_1*/
        return decision;
    }

    





    
    public static AuthorizationDecision decide(Request req) {
        AuthorizationDecision decision = AuthorizationDecision.DENY;
        if (req.headerSecret != null && req.headerSecret.equals("internal")) { // 行1：伪造输入入口
            decision = AuthorizationDecision.ALLOW;
        } else if (req.url != null && req.url.startsWith("/internal/")) {
            decision = AuthorizationDecision.ALLOW;
        }
        /*ANCHOR_2*/
        return decision; // 行2：return ALLOW
    }
}
