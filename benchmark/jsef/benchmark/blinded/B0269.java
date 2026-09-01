package blinded;

import java.util.concurrent.ConcurrentHashMap;
import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RequestParam;
import org.springframework.web.bind.annotation.RestController;




@RestController
public class ConcurrentSessionUnlimitedBy {

    private final ConcurrentHashMap<String, Integer> activeSessions = new ConcurrentHashMap<>();
    private static final int MAX_SESSIONS = 1;

    @PostMapping("/api/v1/session/by/concurrent")
    public String login(@RequestParam String user) {
        int count = activeSessions.merge(user, 1, Integer::sum);
        /*ANCHOR_1*/
        if (count > MAX_SESSIONS) {
            activeSessions.put(user, MAX_SESSIONS); // 超出使旧会话失效
            return "oldest session invalidated";
        }
        return "active sessions: " + count;
    }
}
