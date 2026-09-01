package blinded;

import java.util.logging.Logger;









public class LogInjectionBasicBy {

    private static final Logger logger = Logger.getLogger(LogInjectionBasicBy.class.getName());

    public void login(String username) {
        /*ANCHOR_1*/
        logger.info("User login attempt: {0}", username);
    }

    public static void main(String[] args) {
        new LogInjectionBasicBy().login("alice");
    }
}
