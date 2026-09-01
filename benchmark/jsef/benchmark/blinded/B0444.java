
package blinded;

import javax.servlet.Filter;
import javax.servlet.FilterChain;
import javax.servlet.ServletRequest;
import javax.servlet.ServletResponse;
import javax.servlet.http.HttpServletRequest;
import javax.servlet.http.HttpServletResponse;

import org.springframework.stereotype.Component;
import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.RestController;















public class WebCacheDeceptionBx {

    // ------------------------------------------------------------------
    // 状态机①：鉴权过滤器按 URL 后缀 .css 跳过鉴权（缓存-鉴权耦合）
    // ------------------------------------------------------------------
    @Component
    public static class SuffixBasedAuthFilter implements Filter {

        @Override
        public void doFilter(ServletRequest servletRequest, ServletResponse servletResponse, FilterChain chain) {
            HttpServletRequest req = (HttpServletRequest) servletRequest;
            HttpServletResponse res = (HttpServletResponse) servletResponse;
            String path = req.getRequestURI(); // 原始请求路径，如 /account;.css
            
            if (path.endsWith(".css")) { // 攻击者把动态路由伪装成 .css 后缀
                // 状态机①：未鉴权直接放行到动态 Controller
                chain.doFilter(servletRequest, servletResponse);
                return;
            }
            requireAuth(req, res);
            chain.doFilter(servletRequest, servletResponse);
        }

        void requireAuth(HttpServletRequest req, HttpServletResponse res) {
            // 真实鉴权逻辑（被 .css 后缀判断短路，动态路由同样未鉴权）
        }
    }

    // ------------------------------------------------------------------
    // 状态机②：路由归一化 —— 剥离分号段后 /account;.css 归一为 /account
    // ------------------------------------------------------------------
    @RestController
    public static class AccountController {

        @GetMapping("/account")
        public String account(HttpServletRequest req) {
            String path = req.getRequestURI();
            String normalized = path.replaceAll(";.*$", ""); // 剥离分号段
            if (!"/account".equals(normalized)) {
                return "404";
            }
            String body = loadAccountBody(req); // 敏感响应体（未鉴权可达）
            /*ANCHOR_1*/
            return body; // 状态机②：动态路由返回 200 敏感内容
        }

        // ------------------------------------------------------------------
        // 状态机③：反向代理按 URL 静态后缀 .css 做缓存键，缓存敏感响应体
        // ------------------------------------------------------------------
        String loadAccountBody(HttpServletRequest req) {
            
            String cacheKey = req.getRequestURI(); // 缓存键 = 原始 URI（含 .css）
            return "name=alice;balance=10000;tx=recent" + " [cache-key=" + cacheKey + "]";
        }
    }
}
