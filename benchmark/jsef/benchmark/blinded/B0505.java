
// 漏洞样本：硬编码凭证——硬编码的 SMTP 服务密码
// 漏洞点：邮件服务凭证直接写入代码字面量。
package blinded;

import org.springframework.web.bind.annotation.*;




@RestController
@RequestMapping("/benchmark/bz/hardcoded-credentials")
public class HardcodedCredentialsBxB {

    


    @GetMapping("/unby/send-mail")
    public String unbySendMail(@RequestParam String to) {
        // 危险实践：SMTP 凭证硬编码在代码中
        String smtpHost = "smtp.example.com";
        String smtpUser = "noreply@example.com";
        String smtpPassword = "SmtpPass!2023";
        /*ANCHOR_1*/
        return "{\"msg\":\"邮件已发送（使用硬编码 SMTP 密码）\"}";
    }
}
