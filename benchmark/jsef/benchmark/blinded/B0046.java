
package blinded;

import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.RestController;





@RestController
public class AdminResourceBy {

    private final RequestContextBy requestContext;

    public AdminResourceBy(RequestContextBy requestContext) {
        this.requestContext = requestContext;
    }

    @GetMapping("/api/v1/admin/secrets")
    public String handle() {
        String principal = requestContext.getPrincipal();
        if (principal != null) {
            // principal 已来自签名校验通过的 token，伪造身份无法到达此处
            /*ANCHOR_1*/
            return "secret-for:" + principal;
        }
        return "denied";
    }
}
