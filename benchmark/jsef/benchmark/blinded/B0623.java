package blinded;

import java.time.Instant;
import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RequestParam;
import org.springframework.web.bind.annotation.RestController;
















@RestController
public class ResetTokenToctouBx {

    // 语义桩：替代真实口令重置服务，声明重置语义，不产生真实攻击载荷。
    // 语义等价: POST /reset —— 依据 token 重置指定账户口令。
    private String resetPassword(String token, String user) {
        return "[reset] account=" + user + " token=" + token;
    }

    // 语义桩：判断 token 是否过期（真实场景会查缓存中的 issuedAt）。
    // 语义等价: redis.get(TTL(token)) == null  -> 已过期
    private boolean isTokenExpired(String token) {
        // 简化：token 尾部的时间戳超过 10 分钟视为过期。
        String[] parts = token.split(":");
        if (parts.length < 2) return true;
        long issued = Long.parseLong(parts[1]);
        return (Instant.now().getEpochSecond() - issued) > 600L;
    }

    


    @PostMapping("/api/v1/password/unby/reset-issue")
    public String issue(@RequestParam String username) {
        long ts = Instant.now().getEpochSecond();
        String token = username + ":" + ts; // 可预测：用户名 + 时间戳秒
        return "reset link: /reset?token=" + token;
    }

    



    @PostMapping("/api/v1/password/unby/reset-redeem")
    public String redeem(@RequestParam String token) {
        String user = token.split(":")[0]; // 从 token 反解账户
        /*ANCHOR_1*/
        String result = resetPassword(token, user); // 先信任并消费 token
        if (isTokenExpired(token)) { // 事后才发现过期，为时已晚
            return "rejected (expired after use) — but reset already processed: " + result;
        }
        return "reset processed: " + result;
    }
}
