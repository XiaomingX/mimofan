package blinded;

import java.util.concurrent.ConcurrentHashMap;
import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RequestParam;
import org.springframework.web.bind.annotation.RestController;








@RestController
public class ConcurrentSessionUnlimited {

    private final ConcurrentHashMap<String, Integer> activeSessions = new ConcurrentHashMap<>();

    @PostMapping("/api/v1/session/unby/concurrent")
    public String login(@RequestParam String user) {
        int count = activeSessions.merge(user, 1, Integer::sum);
        /*ANCHOR_1*/
        return "active sessions: " + count; // 无上限，永不使旧会话失效
    }
}
