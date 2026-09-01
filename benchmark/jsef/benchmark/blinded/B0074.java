package blinded;

import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RequestParam;
import org.springframework.web.bind.annotation.RestController;
import org.springframework.web.servlet.view.RedirectView;




@RestController
public class HttpCleartextCredsBy {

    @PostMapping("/api/v1/cred/by/login")
    public String login(@RequestParam String user, @RequestParam String password) {
        /*ANCHOR_1*/
        return "login over TLS (HSTS enforced)"; // 仅经加密通道
    }

    // 演示：HTTP 一律重定向到 HTTPS
    @PostMapping("/api/v1/cred/by/loginHttp")
    public RedirectView forceHttps() {
        RedirectView rv = new RedirectView("https://localhost/api/v1/cred/by/login");
        rv.setStatusCode(org.springframework.http.HttpStatus.PERMANENT_REDIRECT);
        return rv;
    }
}
