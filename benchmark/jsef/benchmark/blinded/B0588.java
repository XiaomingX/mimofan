package blinded;

/*
 * JSEF-Benchmark L3 — 安全过滤器路径匹配绕过（矩阵变量 / 分号段，CWE-863）
 *
 * 难度：L3（两个状态机不一致）。安全过滤器按 path.endsWith(".css") 命中即跳过鉴权，
 * 但 Spring 路由侧会把分号段（矩阵变量）从路径中剥掉再归一化：
 *     /admin;x.css  →  过滤器看到 ".css" 后缀 → 跳过鉴权
 *                    →  Spring 归一化后 /admin → 命中管理员接口，返回敏感操作
 * “过滤器看到的原始路径”与“框架归一化后的路径”不一致，形成鉴权绕过。
 * LLM 容易只关注一端状态机而误判为 BX。
 *
 * CWE-863 (Incorrect Authorization)。
 * 安全底线：仅 localhost 演示语义，不提供真实利用脚本。
 *
 * 修复要点（对照 MatrixPathAuthzBy.java）：先按 UrlPathHelper / PathContainer
 * 归一化（去分号段）再做精确白名单匹配，且鉴权不依赖资源后缀。
 */
public class MatrixPathAuthzBx {

    private static boolean adminAllowed(String normalized) {
        return normalized.equals("/admin");
    }

    




    public boolean authorize(String rawPath) {
        if (rawPath.endsWith(".css")) {              // 过滤器：静态资源 → 跳过鉴权
            /*ANCHOR_1*/
            return true;                             
        }
        String normalized = rawPath.split(";")[0];   // 分号段归一化：/admin;x.css → /admin
        return adminAllowed(normalized);             // 命中管理员接口，返回敏感操作
    }
}
