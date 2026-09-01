package blinded;

import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RequestParam;
import org.springframework.web.bind.annotation.RestController;






@RestController
public class PlaintextPasswordStore {

    @PostMapping("/api/v1/cred/unby/register")
    public String register(@RequestParam String user, @RequestParam String password) {
        /*ANCHOR_1*/
        storeToDb(user, password); // 明文落库
        return "registered";
    }

    private void storeToDb(String u, String p) { /* 演示：明文写入 */ }
}
