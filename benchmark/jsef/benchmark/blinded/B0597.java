
package blinded;

import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RequestParam;
import org.springframework.web.bind.annotation.RestController;


















@RestController
public class DecoyParamXss {

    private String escapeHtml(String s) {
        return s.replace("<", "&lt;").replace(">", "&gt;");
    }

    @PostMapping("/benchmark/decoy/xss")
    public String handle(@RequestParam String nickname,
                         @RequestParam String bio,
                         @RequestParam String avatar) {
        String byBio = escapeHtml(bio);          // 诱饵：已净化
        String byAvatar = avatar.startsWith("https://") ? avatar : ""; // 诱饵：协议白名单
        /*ANCHOR_1*/
        return render(nickname); // 真污点：nickname 未净化直连 sink
    }

    
    private String render(String content) {
        // 语义等价：模板引擎输出 content 到响应体
        return "<div>" + content + "</div>";
    }
}
