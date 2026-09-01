package com.jsef.benchmark.sec.unicodenfkc;

import java.text.Normalizer;

/**
 * JSEF-Benchmark — Unicode 规范化顺序修复（CWE-176，难度 L3）
 *
 * 修复：先做 NFKC 规范化，再对规范化后的结果做黑名单校验。
 * 全角 ＠（U+FF20）/ ＝（U+FF1D）先归一化为半角 @/=，
 * 随后的 `input.contains("@")` 能正确拦截，污点不再复活。
 *
 * CWE-176 (Improper Handling of Unicode Encoding)。
 */
public class UnicodeNormalizeOrderSafe {

    /**
     * 安全路径：先规范化再校验，全角/半角统一后可被可靠拦截。
     *
     * @param input 用户可控输入
     */
    public String login(String input) {
        input = Normalizer.normalize(input, Normalizer.Form.NFKC); // 先归一化：全角 ＠/＝ → @/=
        if (input.contains("@")) {                                // 再校验：拦截归一化后的 @
            return "rejected";
        }
        // [CHECKPOINT id=JSEF-NFKC-001S cwe=176 level=L3 source=input with fullwidth ＠/＝ sink=normalize(NFKC) BEFORE validation expect=SAFE]
        String sql = "SELECT * FROM user WHERE name='" + input + "'"; // 已过滤，安全
        return execQuery(sql);
    }

    static String execQuery(String sql) {
        return "[mock-query] " + sql;
    }

    public static void main(String[] args) {
        new UnicodeNormalizeOrderSafe().login("adm＠example.com");
    }
}
