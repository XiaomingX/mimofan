package blinded;

import jakarta.servlet.http.HttpServletRequest;
import jakarta.servlet.http.HttpSession;
import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RequestParam;
import org.springframework.web.bind.annotation.RestController;




@RestController
public class SessionFixationBy {

    @PostMapping("/api/v1/session/by/fixation")
    public String login(@RequestParam String user, HttpServletRequest request) {
        HttpSession session = request.getSession();
        /*ANCHOR_1*/
        request.changeSessionId(); // 登录后复活会话标识
        session.setAttribute("authenticatedUser", user);
        return "authenticated (new session id)";
    }
}
