/*
 * JSEF Benchmark — FP 混淆型安全样本的对照 bx（CWE-89, 难度 L3）
 *
 * 对照样本：source 为 List<String>（字符串类型），元素可直接拼接进 SQL，
 * 形成字符串注入。与 NumericTypeSqlBy 的 List<Long> 形成 bx/by 对。
 * 安全底线：所有 Payload 仅 localhost 演示语义，不写真实利用脚本。
 */
package blinded;

import java.sql.Statement;
import java.util.List;

public class StringTypeSqlInjection {

    



    static void unby(Statement stmt, List<String> values) throws Exception {
        StringBuilder in = new StringBuilder();
        for (String v : values) {
            in.append("'").append(v).append("',");
        }
        String sql = "SELECT * FROM users WHERE name IN (" + in + "'x')";
        /*ANCHOR_1*/
        stmt.execute(sql);
    }
}
