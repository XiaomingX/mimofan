package blinded;

import org.slf4j.Logger;
import org.slf4j.LoggerFactory;








public class LogInjectionMultilineBy {

    private static final Logger log = LoggerFactory.getLogger(LogInjectionMultilineBy.class);

    public void search(String keyword) {
        String sanitized = keyword.replaceAll("[\\r\\n]", "");
        String msg = "search query executed: " + sanitized;
        /*ANCHOR_1*/
        log.info(msg);
    }

    public static void main(String[] args) {
        new LogInjectionMultilineBy().search("laptop");
    }
}
