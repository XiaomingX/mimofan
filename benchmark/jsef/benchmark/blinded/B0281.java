/*
 * JSEF Benchmark — Phase 4 多后端注入变体（安全对照）
 * 样本 6-S：NamedParameterJdbcTemplate 白名单列名（CWE-89, 难度 L2）
 *
 * 与 JdbcNamedParamAbuse 配对：排序列名经白名单校验后使用常量，值用 :param
 * 参数绑定，用户输入不进入 SQL 文本，故 expect=BX。
 * 安全底线：所有 Payload 仅 localhost 演示语义，不写真实利用脚本。
 */
package blinded;

import java.util.List;
import java.util.Map;

public class JdbcNamedParamAbuseBy {

    


    static List<Map<String, Object>> by(String sortColumn) {
        if (!List.of("id", "username", "email").contains(sortColumn)) {
            throw new IllegalArgumentException("invalid sort column");
        }
        /*ANCHOR_1*/
        return java.util.List.of();
    }

    public static void main(String[] args) {
        System.out.println("JdbcNamedParamAbuseBy demo (localhost only)");
    }
}
