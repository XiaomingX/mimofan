package blinded;












public class MissingLoginFailLog {

    


    public static boolean login(String user, String pwd) {
        boolean ok = "secret".equals(pwd);
        if (!ok) {
            // source：认证失败事件
            /*ANCHOR_1*/
            return false;   // 静默返回，登录失败无记录
        }
        return true;
    }
}
