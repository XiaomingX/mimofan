package blinded;

import jakarta.mail.Message;
import jakarta.mail.internet.InternetAddress;
import jakarta.mail.internet.MimeMessage;









public class MailHeaderInjectionBy {

    
    static String stripCrlf(String value) {
        return value == null ? "" : value.replaceAll("[\\r\\n\\x00-\\x1F]", "");
    }

    
    static boolean isValidHeaderValue(String value) {
        return value != null && value.chars().allMatch(c -> c >= 0x20 && c <= 0x7E);
    }

    
    static boolean isManagedRecipient(String addr) {
        return addr != null && addr.endsWith("@example.com");
    }

    





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
        /*ANCHOR_1*/
        msg.setSubject(subject); // 已无 CR/LF，无法注入额外头字段
        msg.setRecipients(Message.RecipientType.TO, new InternetAddress(recipient));
    }
}
