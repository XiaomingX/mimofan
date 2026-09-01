package blinded;

import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.RequestParam;
import org.springframework.web.bind.annotation.RestController;









@RestController
public class ChainSqlControllerBy {

    private final ChainSqlServiceBy service;

    public ChainSqlControllerBy(ChainSqlServiceBy service) {
        this.service = service;
    }

    @GetMapping("/benchmark/chainsql/by")
    public String handle(@RequestParam String input) {
        /*ANCHOR_1*/
        return service.process(input); // 链路末端以 PreparedStatement 参数化执行
    }
}
