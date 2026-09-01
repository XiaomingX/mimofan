
// 安全对照：硬编码 SMTP 凭证（修复版）
// 修复原则：SMTP 凭证来自环境变量/密钥管理，不硬编码。
package blinded;

import org.springframework.web.bind.annotation.*;




@RestController
@RequestMapping("/benchmark/bx/hardcoded-credentials")
public class HardcodedCredentialsByB {

    


    @GetMapping("/by/send-mail")
    public String bySendMail(@RequestParam String to) {
        String smtpHost = System.getenv("SMTP_HOST");
        String smtpUser = System.getenv("SMTP_USER");
        String smtpPassword = System.getenv("SMTP_PASSWORD");
        if (smtpPassword == null) {
            return "{\"msg\":\"SMTP 凭证未配置\"}";
        }
        // 安全实践：凭证外部化，代码中无硬编码
        /*ANCHOR_1*/
        return "{\"msg\":\"邮件已发送（凭证来自外部配置）\"}";
    }
}
