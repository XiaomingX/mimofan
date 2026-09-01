package blinded;

import java.time.Instant;






public class InadequateLogContentBy {

    


    public static void onAction(String user, String clientIp, String action) {
        // source：安全相关动作事件
        /*ANCHOR_1*/
        System.out.println("[AUDIT] action=" + action
                + " user=" + user + " ip=" + clientIp + " at=" + Instant.now());
    }
}
