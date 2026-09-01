package blinded;

import jakarta.servlet.http.HttpSession;
import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RequestParam;
import org.springframework.web.bind.annotation.RestController;







@RestController
public class SessionNoTimeout {

    @PostMapping("/api/v1/session/unby/notimeout")
    public String login(@RequestParam String user, HttpSession session) {
        session.setAttribute("user", user);
        /*ANCHOR_1*/
        session.setMaxInactiveInterval(-1); // -1 = 永不过期
        return "logged in";
    }
}
