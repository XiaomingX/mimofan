/*
 * JSEF Benchmark 样本 — 授权通配修复（AntPathMatcher 深度语义，CWE-863，L3）
 *
 * 修复：改用多段通配 /admin



package blinded;

import org.springframework.util.AntPathMatcher;

public class AntPatternShallowBy {

    private static final AntPathMatcher MATCHER = new AntPathMatcher();

    // 多段通配：覆盖 /admin 下任意深度子路径（含 /admin/report/export）
    private static final String PROTECTED = "/admin/**";

    private static boolean isAuthenticated() {
        return false;
    }

    static String exportReport(String uri) {
        return "report:" + uri;
    }

    




    public String authorize(String requestUri) {
        // /admin











