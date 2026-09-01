// [VULN]
package com.jsef.benchmark.vuln.hostheader;

import javax.servlet.http.HttpServletRequest;

import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RequestParam;
import org.springframework.web.bind.annotation.RestController;

/**
 * JSEF-Benchmark — Host 头投毒 → 密码重置链接劫持（CWE-601，L3）
 *
 * 密码重置流程：重置链接 base 直接取 Host 头拼接，未经任何校验。
 * 攻击者把 Host 设为 evil.com，受害者点击邮件里的重置链接即跳向攻击者
 * 域名的伪造重置页，攻击者借机窃取重置 token —— CWE-601 开放重定向
 * 语义：链接 base 由攻击者可控。
 *
 * 修复要点（对照 HostHeaderResetSafe.java）：固定配置 base URL + Host
 * 白名单校验 + java.net.URI 严格解析后才拼链接。
 */
@RestController
public class HostHeaderResetVuln {

    @PostMapping("/api/v1/password/unsafe/reset-link")
    public String sendResetLink(@RequestParam String email, HttpServletRequest request) {
        String token = generateToken(email); // 伪随机 token
        // [VULN] 直接取不可信 Host 头作为重置链接 base
        String base = "https://" + request.getHeader("Host"); // 读 Host 头
        // [CHECKPOINT id=JSEF-HOSTRESET-001 cwe=601 level=L3 source=Host header sink=reset link base from attacker-controlled Host expect=VULN trace=benchmark/cases/vuln/host-header-reset/HostHeaderResetVuln.java:28,benchmark/cases/vuln/host-header-reset/HostHeaderResetVuln.java:30,benchmark/cases/vuln/host-header-reset/HostHeaderResetVuln.java:31]
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
