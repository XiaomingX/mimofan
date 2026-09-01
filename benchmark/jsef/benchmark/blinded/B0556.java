package blinded;

import java.util.logging.Logger;















public class LogInjectionBasic {

    private static final Logger logger = Logger.getLogger(LogInjectionBasic.class.getName());

    




    public void login(String username) {
        // 模拟登录处理...
        /*ANCHOR_1*/
        logger.info("User login attempt: " + username);
    }

    public static void main(String[] args) {
        new LogInjectionBasic().login("alice");
    }
}
