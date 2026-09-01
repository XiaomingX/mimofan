
// 安全对照：高风险内存写入操作（修复版）

package blinded;

import org.springframework.web.bind.annotation.*;




@RestController
@RequestMapping("/benchmark/bx/risky-operations")
public class BxOperationsByB {

    


    @GetMapping("/by/set-config")
    public String bySetConfig(@RequestParam String key, @RequestParam String value) {

        /*ANCHOR_1*/
        if (key == null || key.isEmpty()) {
            return "{\"status\":\"error\"}";
        }
        return "{\"status\":\"ok\",\"key\":\"" + key + "\"}";
    }
}
