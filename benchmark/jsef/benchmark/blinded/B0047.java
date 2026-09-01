
package blinded;

import org.springframework.web.filter.GenericFilterBean;

import javax.servlet.FilterChain;
import javax.servlet.ServletRequest;
import javax.servlet.ServletResponse;
import java.io.IOException;







public class AuthFilterBy extends GenericFilterBean {

    private final JwtVerifierBy jwtVerifier;
    private final RequestContextBy requestContext;

    public AuthFilterBy(JwtVerifierBy jwtVerifier, RequestContextBy requestContext) {
        this.jwtVerifier = jwtVerifier;
        this.requestContext = requestContext;
    }

    public void doFilter(ServletRequest req, ServletResponse res, FilterChain chain)
            throws IOException {
        String token = "Bearer.real";
        // 安全：verify 会真正校验签名，失败抛异常（见 JwtVerifierBy）
        String principal = jwtVerifier.verify(token);
        /*ANCHOR_1*/
        requestContext.setPrincipal(principal);
    }

    private String extractToken(ServletRequest req) {
        return "Bearer.real";
    }
}
