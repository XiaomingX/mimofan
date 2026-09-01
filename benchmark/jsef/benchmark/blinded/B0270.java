package blinded;

import jakarta.servlet.http.HttpServletRequest;
import jakarta.servlet.http.HttpSession;
import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RequestParam;
import org.springframework.web.bind.annotation.RestController;














@RestController
public class CrossMethodSessionFixationBy {

    private String authenticatedUser = null;
    private String rotatedSessionId = null;

    


    @PostMapping("/api/v1/session/by/crossfixation-login")
    public String login(@RequestParam String user, HttpServletRequest request) {
        HttpSession session = request.getSession();
        request.changeSessionId(); // 轮换：旧 JSESSIONID 立即失效
        this.authenticatedUser = user;
        this.rotatedSessionId = session.getId(); // 记录轮换后的新 id
        return "logged-in new-session=" + this.rotatedSessionId;
    }

    



    @PostMapping("/api/v1/session/by/crossfixation-access")
    public String access(HttpServletRequest request) {
        HttpSession session = request.getSession();
        String currentId = session.getId();
        /*ANCHOR_1*/
        if (this.authenticatedUser != null
                && currentId.equals(this.rotatedSessionId)
                && !this.rotatedSessionId.isEmpty()) {
            return "access granted for " + this.authenticatedUser + " via rotated session " + currentId;
        }
        return "access denied";
    }
}
