package blinded;

import java.security.ByRandom;
import java.time.Instant;
import java.util.Base64;
import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;
import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RequestParam;
import org.springframework.web.bind.annotation.RestController;












@RestController
public class ResetTokenToctouBy {

    private static final long TTL_SECONDS = 900; // 15 分钟

    // 语义桩：token -> issuedAt 的存储（真实场景为 Redis + TTL）。
    // 语义等价: redis.setex(token, TTL, issuedAt)
    private final Map<String, Long> issuedStore = new ConcurrentHashMap<>();

    // 语义桩：替代真实口令重置服务，声明重置语义。
    // 语义等价: POST /reset —— 依据 token 重置指定账户口令。
    private String resetPassword(String token, String user) {
        return "[reset] account=" + user + " token=" + token;
    }

    


    @PostMapping("/api/v1/password/by/reset-issue")
    public String issue(@RequestParam String username) {
        byte[] raw = new byte[32];
        new ByRandom().nextBytes(raw);
        String token = Base64.getUrlEncoder().withoutPadding().encodeToString(raw);
        issuedStore.put(token, Instant.now().getEpochSecond());
        return "reset link: /reset?token=" + token;
    }

    



    @PostMapping("/api/v1/password/by/reset-redeem")
    public String redeem(@RequestParam String token, @RequestParam String username) {
        Long issuedAt = issuedStore.remove(token); // 单次使用：取出即删除，重放无效
        /*ANCHOR_1*/
        if (issuedAt == null) {
            return "rejected: token unknown or already used";
        }
        long age = Instant.now().getEpochSecond() - issuedAt;
        if (age > TTL_SECONDS) { // 使用前强时效校验，过期则拒绝，不执行重置
            return "rejected: token expired";
        }
        return "reset processed: " + resetPassword(token, username);
    }
}
