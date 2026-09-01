package blinded;

import java.util.HashSet;
import java.util.Set;
import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RequestParam;
import org.springframework.web.bind.annotation.RestController;




@RestController
public class ResetKeepsOldSessionBy {

    private final Set<String> activeSessions = new HashSet<>();

    @PostMapping("/api/v1/password/by/doReset")
    public String doReset(@RequestParam String username, @RequestParam String newPassword) {
        // 假设已更新口令
        /*ANCHOR_1*/
        activeSessions.remove(username); // 使旧会话失效
        return "password updated; old sessions invalidated";
    }
}
