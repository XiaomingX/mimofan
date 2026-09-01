package blinded;

import jakarta.mail.Message;
import jakarta.mail.internet.InternetAddress;
import jakarta.mail.internet.MimeMessage;

















public class MailHeaderInjectionBx {

    





    public void send(String toAddr, String userName) throws Exception {
        MimeMessage msg = new MimeMessage(null);
        String subject = "Password reset for " + userName; // 中间变量 1：污点进入 Subject
        String recipient = toAddr;                         // 中间变量 2：污点进入 Recipient
        /*ANCHOR_1*/
        msg.setSubject(subject); 
        msg.setRecipients(Message.RecipientType.TO, new InternetAddress(recipient)); // sink：可注入 Bcc / 伪造发件人
    }
}
