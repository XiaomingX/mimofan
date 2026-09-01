package blinded;

import org.slf4j.Logger;
import org.slf4j.LoggerFactory;














public class LogInjectionMultiline {

    private static final Logger log = LoggerFactory.getLogger(LogInjectionMultiline.class);

    




    public void search(String keyword) {
        String msg = "search query executed: " + keyword;
        // 中间处理...
        /*ANCHOR_1*/
        log.info(msg);
    }

    public static void main(String[] args) {
        new LogInjectionMultiline().search("laptop");
    }
}
