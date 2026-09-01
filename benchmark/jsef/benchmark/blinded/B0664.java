package blinded;

import jakarta.servlet.http.HttpServletRequest;
import jakarta.servlet.http.HttpSession;
import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RequestParam;
import org.springframework.web.bind.annotation.RestController;







@RestController
public class SessionFixation {

    @PostMapping("/api/v1/session/unby/fixation")
    public String login(@RequestParam String user, HttpServletRequest request) {
        HttpSession session = request.getSession();
        session.setAttribute("authenticatedUser", user);
        /*ANCHOR_1*/
        return "authenticated (same session id)"; // 未调用 changeSessionId
    }
}
