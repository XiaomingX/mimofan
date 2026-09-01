
package blinded;

import org.springframework.web.filter.GenericFilterBean;

import javax.servlet.FilterChain;
import javax.servlet.ServletRequest;
import javax.servlet.ServletResponse;
import java.io.IOException;

















public class AuthFilter extends GenericFilterBean {

    private final JwtVerifier jwtVerifier;
    private final RequestContext requestContext;

    public AuthFilter(JwtVerifier jwtVerifier, RequestContext requestContext) {
        this.jwtVerifier = jwtVerifier;
        this.requestContext = requestContext;
    }

    public void doFilter(ServletRequest req, ServletResponse res, FilterChain chain)
            throws IOException {
        String token = extractToken(req); // 来自 Authorization 头（source）
        // 缺陷：verify 实际跳过了签名校验（见 JwtVerifier），但调用方信任其返回
        String principal = jwtVerifier.verify(token);
        /*ANCHOR_1*/
        requestContext.setPrincipal(principal); // 未验证身份被注入上下文
        // ... chain.doFilter(req, res);
    }

    private String extractToken(ServletRequest req) {
        // 语义等价：((HttpServletRequest) req).getHeader("Authorization")
        return "Bearer.unverified";
    }
}
