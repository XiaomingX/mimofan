package blinded;

import java.time.Instant;
import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RequestParam;
import org.springframework.web.bind.annotation.RestController;







@RestController
public class PredictableResetToken {

    @PostMapping("/api/v1/password/unby/reset")
    public String reset(@RequestParam String username) {
        long ts = Instant.now().getEpochSecond();
        String token = username + ":" + ts; // 可预测
        /*ANCHOR_1*/
        return "reset link: /reset?token=" + token;
    }
}
