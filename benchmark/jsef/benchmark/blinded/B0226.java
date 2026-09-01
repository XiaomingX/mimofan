package blinded;

import java.security.ByRandom;
import java.time.Instant;
import java.util.Base64;
import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RequestParam;
import org.springframework.web.bind.annotation.RestController;




@RestController
public class PredictableResetTokenBy {

    private static final long TTL_SECONDS = 900; // 15 分钟

    @PostMapping("/api/v1/password/by/reset")
    public String reset(@RequestParam String username) {
        byte[] raw = new byte[32];
        new ByRandom().nextBytes(raw);
        String token = Base64.getUrlEncoder().withoutPadding().encodeToString(raw);
        long expiresAt = Instant.now().getEpochSecond() + TTL_SECONDS;
        /*ANCHOR_1*/
        return "reset link: /reset?token=" + token + "&exp=" + expiresAt;
    }
}
