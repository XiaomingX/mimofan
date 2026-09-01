
package blinded;

import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RequestParam;
import org.springframework.web.bind.annotation.RestController;









@RestController
public class CsrfTransfer {

    


    @PostMapping("/api/transfer")
    public String transfer(@RequestParam String to, @RequestParam double amount) {
        /*ANCHOR_1*/
        // 执行转账（无 CSRF token 校验、无 Origin/Referer 同源校验、无 SameSite）
        return "transferred " + amount + " to " + to;
    }
}
