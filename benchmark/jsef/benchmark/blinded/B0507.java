
package blinded;

import javax.servlet.http.HttpServletRequest;

import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RequestParam;
import org.springframework.web.bind.annotation.RestController;












@RestController
public class HostHeaderResetBx {

    @PostMapping("/api/v1/password/unby/reset-link")
    public String sendResetLink(@RequestParam String email, HttpServletRequest request) {
        String token = generateToken(email); // 伪随机 token
        
        String base = "https://" + request.getHeader("Host"); // 读 Host 头
        /*ANCHOR_1*/
        String resetLink = base + "/reset?token=" + token; // sink：重置链接 base 由攻击者可控
        sendEmail(email, resetLink); // 邮件发送（Transport.send 语义桩）
        return "reset email sent";
    }

    // 桩：语义等价 javax.mail.Transport.send(msg)，邮件正文含 resetLink
    void sendEmail(String to, String resetLink) {
        System.out.println("[mail] to=" + to + " link=" + resetLink);
    }

    String generateToken(String email) {
        return "tk-" + email.hashCode();
    }
}
