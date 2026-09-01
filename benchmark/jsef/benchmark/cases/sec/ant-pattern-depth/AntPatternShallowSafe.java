/*
 * JSEF Benchmark 样本 — 授权通配修复（AntPathMatcher 深度语义，CWE-863，L3）
 *
 * 修复：改用多段通配 /admin/**（覆盖 /admin/report/export 等深层路径），
 * 并在未命中受保护前缀时默认拒绝（deny by default）。
 * 对照 AntPatternShallowVuln.java：/admin/* 只匹配单段导致越权直通。
 */
package com.jsef.benchmark.sec;

import org.springframework.util.AntPathMatcher;

public class AntPatternShallowSafe {

    private static final AntPathMatcher MATCHER = new AntPathMatcher();

    // 多段通配：覆盖 /admin 下任意深度子路径（含 /admin/report/export）
    private static final String PROTECTED = "/admin/**";

    private static boolean isAuthenticated() {
        return false;
    }

    static String exportReport(String uri) {
        return "report:" + uri;
    }

    /**
     * 授权过滤器：/admin/** 命中深层请求；未命中或未认证一律拒绝。
     *
     * @param requestUri 请求 URI，如 /admin/report/export
     */
    public String authorize(String requestUri) {
        // /admin/** 命中深层路径：/admin/report/export 亦在保护范围内
        boolean matched = MATCHER.match(PROTECTED, requestUri);
        if (!matched) {
            return "403 FORBIDDEN"; // 未命中受保护前缀 → 默认拒绝
        }
        if (!isAuthenticated()) {
            return "401 UNAUTHORIZED"; // 命中且未认证 → 拦截
        }
        // [CHECKPOINT id=JSEF-ANTPAT-001S cwe=863 level=L3 source=request URI /admin/report/export sink=deep wildcard /admin/** blocks unauthenticated expect=SAFE]
        return exportReport(requestUri); // 认证通过 → 正常放行
    }
}
