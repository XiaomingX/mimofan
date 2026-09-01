
// 安全对照：高风险操作（修复版）


package blinded;

import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.RequestMapping;
import org.springframework.web.bind.annotation.RequestParam;
import org.springframework.web.bind.annotation.RestController;




@RestController
@RequestMapping("/benchmark/bx/unby-operations")
public class BxOperationsBy {

    


    @GetMapping("/by/read-config")
    public String byReadConfig(@RequestParam(required = false) String key) {

        /*ANCHOR_1*/
        if (key == null || key.isEmpty()) {
            return "{\"status\":\"error\",\"message\":\"请提供配置键\"}";
        }
        return "{\"status\":\"ok\",\"key\":\"" + key + "\",\"value\":\"trusted-config\"}";
    }
}
