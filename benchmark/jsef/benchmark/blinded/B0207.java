
package blinded;

import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RequestParam;
import org.springframework.web.bind.annotation.RestController;









@RestController
public class DecoyParamXss_By {

    private String escapeHtml(String s) {
        return s.replace("<", "&lt;").replace(">", "&gt;");
    }

    @PostMapping("/benchmark/decoy/xss/by")
    public String handle(@RequestParam String nickname,
                         @RequestParam String bio,
                         @RequestParam String avatar) {
        String byNick = escapeHtml(nickname);
        String byBio = escapeHtml(bio);
        String byAvatar = avatar.startsWith("https://") ? avatar : "";
        /*ANCHOR_1*/
        return render(byNick + byBio + byAvatar);
    }

    private String render(String content) {
        return "<div>" + content + "</div>";
    }
}
