package blinded;

import java.time.Instant;






public class NoAuditTrailBy {

    


    public static void changePassword(String user, String newPwd) {
        // source：敏感操作事件
        /*ANCHOR_1*/
        System.out.println("[AUDIT] PASSWORD_CHANGE actor=" + user
                + " at=" + Instant.now());
        storePassword(user, newPwd);
    }

    private static void storePassword(String u, String p) { /* ... */ }
}
