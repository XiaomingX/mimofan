package blinded;

import jakarta.servlet.http.HttpSession;
import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RequestParam;
import org.springframework.web.bind.annotation.RestController;




@RestController
public class SessionNoTimeoutBy {

    @PostMapping("/api/v1/session/by/notimeout")
    public String login(@RequestParam String user, HttpSession session) {
        session.setAttribute("user", user);
        /*ANCHOR_1*/
        session.setMaxInactiveInterval(1800); // 30 分钟超时
        return "logged in";
    }
}
