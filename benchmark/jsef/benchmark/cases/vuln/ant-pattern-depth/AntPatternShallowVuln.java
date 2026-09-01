// [VULN]
/*
 * JSEF Benchmark 样本 — 授权通配过浅（AntPathMatcher 深度语义，CWE-863，L3）
 *
 * 用 AntPathMatcher 配置 /admin/* 保护管理接口，但该通配只匹配单段路径：
 *   /admin/report         → 命中（单段），需鉴权
 *   /admin/report/export  → 不命中（两段），被当作公开可访问路径
 * 而 /admin/** 才匹配多段路径。攻击者直接请求 /admin/report/export 即可
 * 免鉴权直通敏感导出接口。属 CWE-863 错误授权（Incorrect Authorization）。
 *
 * 安全底线：仅 localhost 演示语义，不写真实利用脚本。
 * 修复要点（对照 AntPatternShallowSafe.java）：改用 /admin/** 或精确路径集 + 默认拒绝。
 */
package com.jsef.benchmark.vuln;

import org.springframework.util.AntPathMatcher;

public class AntPatternShallowVuln {

    private static final AntPathMatcher MATCHER = new AntPathMatcher();

    // 管理接口保护规则：单段通配，未覆盖深层路径
    private static final String PROTECTED = "/admin/*";

    // 模拟未登录的匿名会话
    private static boolean isAuthenticated() {
        return false;
    }

    // 抽象敏感导出（应受 /admin/* 保护）
    static String exportReport(String uri) {
        return "report:" + uri;
    }

    /**
     * 授权过滤器（等价 Spring Security authorizeRequest().antMatchers(PROTECTED)）。
     *
     * /admin/*  只匹配单段：/admin/report 命中；
     * /admin/report/export 因多出子路径而“未命中”，
     * 被当成匿名可访问路径，直接放行到敏感导出。
     *
     * @param requestUri 请求 URI，如 /admin/report/export
     */
    public String authorize(String requestUri) {
        // node1: AntPathMatcher 匹配规则校验（/admin/* 对多段请求不命中）
        boolean matched = MATCHER.match(PROTECTED, requestUri);
        // node2: 未命中 → 视作无需鉴权，跳过认证
        if (!matched) {
            // node3/sink: 放行敏感导出（免鉴权直通）
            // [CHECKPOINT id=JSEF-ANTPAT-001 cwe=863 level=L3 source=request URI /admin/report/export sink=shallow wildcard /admin/* authorizes too little expect=VULN trace=benchmark/cases/vuln/ant-pattern-depth/AntPatternShallowVuln.java:46,benchmark/cases/vuln/ant-pattern-depth/AntPatternShallowVuln.java:48,benchmark/cases/vuln/ant-pattern-depth/AntPatternShallowVuln.java:51]
            return exportReport(requestUri); // 免鉴权直通敏感导出
        }
        if (!isAuthenticated()) {
            return "401 UNAUTHORIZED";
        }
        return exportReport(requestUri);
    }
}
