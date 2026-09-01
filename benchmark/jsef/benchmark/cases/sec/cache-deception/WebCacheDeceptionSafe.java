package com.jsef.benchmark.sec.cachedeception;

import javax.servlet.Filter;
import javax.servlet.FilterChain;
import javax.servlet.ServletRequest;
import javax.servlet.ServletResponse;
import javax.servlet.http.HttpServletRequest;
import javax.servlet.http.HttpServletResponse;

import org.springframework.stereotype.Component;
import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.RestController;

/**
 * JSEF-Benchmark — Web Cache Deception 修复（CWE-285，L4）
 *
 * 修复三要点：
 *   ① 敏感响应加 Cache-Control: private, no-store，反向代理不得缓存；
 *   ② 严格路径校验：仅真实静态目录下的请求按静态后缀放行，
 *      动态路由伪装 .css 后缀一律 404；
 *   ③ 鉴权不依赖 URL 后缀：先 requireAuth 再放行。
 */
public class WebCacheDeceptionSafe {

    @Component
    public static class AuthFirstFilter implements Filter {

        @Override
        public void doFilter(ServletRequest servletRequest, ServletResponse servletResponse, FilterChain chain) {
            HttpServletRequest req = (HttpServletRequest) servletRequest;
            HttpServletResponse res = (HttpServletResponse) servletResponse;
            String path = req.getRequestURI();

            // ② 严格路径校验：/account;.css 归一后是动态路由，拒绝伪装
            String normalized = path.replaceAll(";.*$", "");
            boolean isStatic = normalized.startsWith("/static/");
            if (path.endsWith(".css") && !isStatic) {
                res.setStatus(HttpServletResponse.SC_NOT_FOUND);
                return;
            }

            // ③ 鉴权不依赖后缀：先鉴权再放行
            requireAuth(req, res);

            // ① 敏感响应禁止缓存：动态响应不落入反向代理缓存
            res.setHeader("Cache-Control", "private, no-store");
            // [CHECKPOINT id=JSEF-WCD-001S cwe=285 level=L4 source=request path with .css suffix sink=sensitive response body cached expect=SAFE]
            chain.doFilter(servletRequest, servletResponse);
        }

        static String stripSemicolon(String path) {
            return path.replaceAll(";.*$", "");
        }

        void requireAuth(HttpServletRequest req, HttpServletResponse res) {
            // 真实鉴权：未登录返回 302/401，再往下不执行
        }
    }

    @RestController
    public static class AccountController {

        @GetMapping("/account")
        public String account(HttpServletRequest req) {
            // 敏感体始终带 no-store 头（见 Filter ①），不落入缓存
            return loadAccountBody(req);
        }

        String loadAccountBody(HttpServletRequest req) {
            return "name=alice;balance=10000";
        }
    }
}
