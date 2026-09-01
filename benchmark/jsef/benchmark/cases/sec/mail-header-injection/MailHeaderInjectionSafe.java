package com.jsef.benchmark.sec;

import jakarta.mail.Message;
import jakarta.mail.internet.InternetAddress;
import jakarta.mail.internet.MimeMessage;

/**
 * JSEF-Benchmark L2 — SMTP 邮件头注入修复（CWE-93）
 *
 * 修复：先剥离 CR/LF 与控制字符，再做 RFC 5322 头字段校验（仅允许可打印 ASCII），
 * 最后仅允许受管收件人地址。污点数据无法以头字段终止符形式进入邮件头，注入被阻断。
 *
 * CWE-93 Improper Neutralization of CRLF Sequences ('CRLF Injection')。
 */
public class MailHeaderInjectionSafe {

    /** 剥离 CR / LF 与所有控制字符，防止头字段终止。 */
    static String stripCrlf(String value) {
        return value == null ? "" : value.replaceAll("[\\r\\n\\x00-\\x1F]", "");
    }

    /** RFC 5322 头字段校验：仅允许可打印 ASCII（0x20-0x7E）。 */
    static boolean isValidHeaderValue(String value) {
        return value != null && value.chars().allMatch(c -> c >= 0x20 && c <= 0x7E);
    }

    /** 受管收件人白名单（演示语义）。 */
    static boolean isManagedRecipient(String addr) {
        return addr != null && addr.endsWith("@example.com");
    }

    /**
     * 安全路径：剥离 CR/LF + 校验头字段 + 仅受管收件人。
     *
     * @param toAddr   用户可控收件人地址
     * @param userName 用户可控显示名
     */
    public void send(String toAddr, String userName) throws Exception {
        String subject = stripCrlf("Password reset for " + userName); // 清洗 Subject
        String recipient = stripCrlf(toAddr);                          // 清洗 Recipient
        if (!isValidHeaderValue(subject) || !isValidHeaderValue(recipient)) {
            throw new IllegalArgumentException("invalid header value");
        }
        if (!isManagedRecipient(recipient)) {
            throw new IllegalArgumentException("unmanaged recipient");
        }
        MimeMessage msg = new MimeMessage(null);
        // [CHECKPOINT id=JSEF-MAILINJ-001S cwe=93 level=L2 source=recipient/subject with CRLF sink=MailMessage.setSubject/setRecipients after sanitize expect=SAFE]
        msg.setSubject(subject); // 已无 CR/LF，无法注入额外头字段
        msg.setRecipients(Message.RecipientType.TO, new InternetAddress(recipient));
    }
}
