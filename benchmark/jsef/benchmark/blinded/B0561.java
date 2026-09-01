package blinded;











public class NoAuditTrail {

    


    public static void changePassword(String user, String newPwd) {
        // source：敏感操作事件
        /*ANCHOR_1*/
        storePassword(user, newPwd);   // 直接落库，无审计
    }

    private static void storePassword(String u, String p) { /* ... */ }
}
