package blinded;












public class InadequateLogContent {

    


    public static void onAction(String user, String clientIp, String action) {
        // source：安全相关动作事件
        /*ANCHOR_1*/
        System.out.println("[INFO] action performed: " + action);   // 缺 user/ip
    }
}
