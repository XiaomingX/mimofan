// [VULN]
package com.jsef.benchmark.vuln.unicodenfkc;

import java.text.Normalizer;

/**
 * JSEF-Benchmark — Unicode 规范化顺序错误（CWE-176，难度 L3）
 *
 * 危险点：先做黑名单校验，再做 NFKC 规范化，顺序颠倒。
 * 攻击者可提交全角 ＠（U+FF20）/ 全角 ＝（U+FF1D），
 * 校验 `input.contains("@")` 只拦半角 @，全角字符漏过；
 * 随后 Normalizer.normalize(input, NFKC) 把全角 ＠/＝ 归一化为
 * 半角 @/=，污点在到达真实 sink（如 SQL / URL 拼接）之前复活。
 * "校验与规范化顺序" 是关键：校验必须作用于规范化之后的输入。
 *
 * CWE-176 (Improper Handling of Unicode Encoding)。
 * 安全底线：仅 localhost 演示语义，不提供真实攻击利用输入。
 *
 * 修复要点（对照 UnicodeNormalizeOrderSafe.java）：先 NFKC 归一化，再校验。
 */
public class UnicodeNormalizeOrderVuln {

    /**
     * 危险路径：先校验后规范化，规范化使校验失效，污点复活。
     *
     * @param input 用户可控输入，可能含全角 ＠/＝
     */
    public String login(String input) {
        if (input.contains("@")) {                 // 校验①：只拦半角 @，全角 ＠/＝ 漏过
            return "rejected";
        }
        // [CHECKPOINT id=JSEF-NFKC-001 cwe=176 level=L3 source=input with fullwidth ＠/＝ sink=normalize(NFKC) AFTER validation revives taint expect=VULN trace=benchmark/cases/vuln/unicode-nfkc/UnicodeNormalizeOrderVuln.java:29,benchmark/cases/vuln/unicode-nfkc/UnicodeNormalizeOrderVuln.java:33,benchmark/cases/vuln/unicode-nfkc/UnicodeNormalizeOrderVuln.java:35]
        input = Normalizer.normalize(input, Normalizer.Form.NFKC); // sink：全角 ＠/＝ 归一化为 @/=，污点复活
        String sql = "SELECT * FROM user WHERE name='" + input + "'"; // 复活后的 @/= 拼入 SQL
        return execQuery(sql);                                       // 污点到达真实危险终点
    }

    static String execQuery(String sql) {
        return "[mock-query] " + sql;
    }

    public static void main(String[] args) {
        new UnicodeNormalizeOrderVuln().login("adm＠example.com");  // 全角 ＠ 演示：仅 localhost 语义
    }
}
