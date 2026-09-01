package blinded;

import org.springframework.security.crypto.bcrypt.BCryptPasswordEncoder;
import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RequestParam;
import org.springframework.web.bind.annotation.RestController;




@RestController
public class PlaintextPasswordStoreBy {

    private final BCryptPasswordEncoder encoder = new BCryptPasswordEncoder();

    @PostMapping("/api/v1/cred/by/register")
    public String register(@RequestParam String user, @RequestParam String password) {
        /*ANCHOR_1*/
        String hash = encoder.encode(password); // 仅存哈希
        storeToDb(user, hash);
        return "registered";
    }

    private void storeToDb(String u, String h) { /* 存哈希值 */ }
}
