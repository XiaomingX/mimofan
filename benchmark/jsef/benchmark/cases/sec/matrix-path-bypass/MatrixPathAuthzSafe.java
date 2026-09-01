package com.jsef.benchmark.sec;

/*
 * JSEF-Benchmark L3 — 安全过滤器路径匹配修复（矩阵变量 / 分号段，CWE-863）
 *
 * 修复：鉴权完全基于归一化路径，不依赖资源后缀。
 *   - 先用与 Spring 一致的语义（UrlPathHelper / PathContainer）剥掉分号段（矩阵变量）
 *   - 归一化后的路径做精确白名单匹配
 *   - 非白名单路径一律拒绝，后缀不再参与鉴权决策
 * /admin;x.css 归一化为 /admin 后，仍会精确命中白名单按真实资源鉴权，
 * 不再因为 ".css" 后缀被跳过。
 *
 * CWE-863 (Incorrect Authorization)。
 */
public class MatrixPathAuthzSafe {

    private static boolean adminAllowed(String normalized) {
        return normalized.equals("/admin");
    }

    /**
     * 基于归一化路径精确鉴权，后缀不再影响决策。
     *
     * @param rawPath 用户可控的原始请求路径
     */
    public boolean authorize(String rawPath) {
        String normalized = rawPath.split(";")[0];   // 去分号段：/admin;x.css → /admin
        if (!adminAllowed(normalized)) {
            return false;                            // 非白名单路径一律拒绝
        }
        // [CHECKPOINT id=JSEF-MATRIX-001S cwe=863 level=L3 source=/admin;x.css style path sink=authorization decision after normalized path allowlist expect=SAFE]
        return true;                                 // 归一化后精确命中白名单才放行
    }
}
