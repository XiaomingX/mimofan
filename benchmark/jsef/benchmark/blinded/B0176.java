package blinded;

import org.apache.logging.log4j.LogManager;
import org.apache.logging.log4j.Logger;
import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.RequestParam;
import org.springframework.web.bind.annotation.RestController;






@RestController
public class Log4j2JndiInjectionBy {

    private static final Logger logger = LogManager.getLogger(Log4j2JndiInjectionBy.class);

    @GetMapping("/api/v1/log4j2/by/log")
    public String log(@RequestParam String userInput) {
        /*ANCHOR_1*/
        logger.info("user action: {}", userInput); // 参数化 + lookup 关闭
        return "logged byly";
    }
}
