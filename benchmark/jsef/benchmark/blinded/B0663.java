package blinded;

import jakarta.servlet.http.HttpServletRequest;
import jakarta.servlet.http.HttpSession;
import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RequestParam;
import org.springframework.web.bind.annotation.RestController;

















@RestController
public class CrossMethodSessionFixation {

    // 两 handler 共享的会话状态：登录方法在此写入身份，access 方法在此校验。
    private String authenticatedUser = null;
    private String currentSessionId = null;

    



    @PostMapping("/api/v1/session/unby/crossfixation-login")
    public String login(@RequestParam String user, HttpServletRequest request) {
        HttpSession session = request.getSession(); // 复用登录前旧 JSESSIONID
        this.authenticatedUser = user;
        this.currentSessionId = session.getId();
        // 关键缺陷：这里应当 request.changeSessionId()，但未调用，
        // 旧会话标识继续有效，供 access 方法复用。
        return "logged-in session=" + this.currentSessionId;
    }

    



    @PostMapping("/api/v1/session/unby/crossfixation-access")
    public String access(HttpServletRequest request) {
        HttpSession session = request.getSession();
        String currentId = session.getId();
        /*ANCHOR_1*/
        if (this.authenticatedUser != null && currentId.equals(this.currentSessionId)) {
            return "access granted for " + this.authenticatedUser + " via session " + currentId;
        }
        return "access denied";
    }
}
