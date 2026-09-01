package blinded;

import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RequestParam;
import org.springframework.web.bind.annotation.RestController;







@RestController
public class HttpCleartextCreds {

    @PostMapping("/api/v1/cred/unby/login")
    public String login(@RequestParam String user, @RequestParam String password) {
        /*ANCHOR_1*/
        return "login over cleartext HTTP"; // 无 TLS 保护
    }
}
