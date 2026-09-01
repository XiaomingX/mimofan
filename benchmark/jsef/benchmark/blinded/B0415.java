
/*
 * JSEF Benchmark 样本 — 授权通配过浅（AntPathMatcher 深度语义，CWE-863，L3）
 *
 * 用 AntPathMatcher 配置 /admin/* 保护管理接口，但该通配只匹配单段路径：
 *   /admin/report         → 命中（单段），需鉴权
 *   /admin/report/export  → 不命中（两段），被当作公开可访问路径
 * 而 /admin





package blinded;

import org.springframework.util.AntPathMatcher;

public class AntPatternShallowBx {

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

    








    public String authorize(String requestUri) {
        // node1: AntPathMatcher 匹配规则校验（/admin/* 对多段请求不命中）
        boolean matched = MATCHER.match(PROTECTED, requestUri);
        // node2: 未命中 → 视作无需鉴权，跳过认证
        if (!matched) {
            // node3/sink: 放行敏感导出（免鉴权直通）
            /*ANCHOR_1*/
            return exportReport(requestUri); // 免鉴权直通敏感导出
        }
        if (!isAuthenticated()) {
            return "401 UNAUTHORIZED";
        }
        return exportReport(requestUri);
    }
}
