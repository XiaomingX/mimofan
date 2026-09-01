package com.jsef.benchmark.sec.hostheader;

import java.net.URI;
import java.util.Set;
import javax.servlet.http.HttpServletRequest;

import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RequestParam;
import org.springframework.web.bind.annotation.RestController;

/**
 * JSEF-Benchmark — Host 头投毒 → 密码重置链接修复（CWE-601，L3）
 *
 * 修复三要点：
 *   ① 固定配置 base URL，重置链接不再取 Host 头；
 *   ② Host 白名单校验：非法 Host 直接拒绝；
 *   ③ java.net.URI 严格解析，仅放行 https 绝对链接后才拼 token。
 */
@RestController
public class HostHeaderResetSafe {

    private static final Set<String> ALLOWED_HOSTS =
            Set.of("app.example.com", "localhost");

    private static final String FIXED_BASE_URL = "https://app.example.com";

    @PostMapping("/api/v1/password/safe/reset-link")
    public String sendResetLink(@RequestParam String email, HttpServletRequest request) {
        String token = generateToken(email);

        // ① 固定配置 base URL，不取 Host 头
        String base = FIXED_BASE_URL;

        // ② Host 白名单校验：Host 仅用于校验，不参与链接拼接
        String host = request.getHeader("Host");
        if (host == null || !ALLOWED_HOSTS.contains(host)) {
            throw new IllegalArgumentException("disallowed Host");
        }

        // ③ URI 严格解析：仅允许 https 绝对 URI
        URI baseUri = URI.create(base);
        if (!baseUri.isAbsolute() || !"https".equals(baseUri.getScheme())) {
            throw new IllegalArgumentException("unsafe base URL");
        }

        // [CHECKPOINT id=JSEF-HOSTRESET-001S cwe=601 level=L3 source=Host header sink=reset link base from attacker-controlled Host expect=SAFE]
        String resetLink = baseUri.toString() + "/reset?token=" + token;
        sendEmail(email, resetLink);
        return "reset email sent";
    }

    void sendEmail(String to, String resetLink) {
        System.out.println("[mail] to=" + to + " link=" + resetLink);
    }

    String generateToken(String email) {
        return "tk-" + email.hashCode();
    }
}
