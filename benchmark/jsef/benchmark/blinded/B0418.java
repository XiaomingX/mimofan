
package blinded;

import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.RestController;








@RestController
public class AdminResource {

    private final RequestContext requestContext;

    public AdminResource(RequestContext requestContext) {
        this.requestContext = requestContext;
    }

    @GetMapping("/api/v1/admin/secrets")
    public String handle() {
        String principal = requestContext.getPrincipal();
        if (principal != null) { // 仅检查非空，不验证真实性
            /*ANCHOR_1*/
            return "secret-for:" + principal; // 伪造身份可触达
        }
        return "denied";
    }
}
