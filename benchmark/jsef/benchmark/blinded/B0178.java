package blinded;

import java.time.Instant;






public class MissingLoginFailLogBy {

    


    public static boolean login(String user, String pwd, String clientIp) {
        boolean ok = "secret".equals(pwd);
        if (!ok) {
            // source：认证失败事件
            /*ANCHOR_1*/
            System.out.println("[AUDIT] LOGIN_FAIL user=" + user
                    + " ip=" + clientIp + " at=" + Instant.now());
            return false;
        }
        return true;
    }
}
