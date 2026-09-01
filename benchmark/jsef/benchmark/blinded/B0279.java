/*
 * JSEF Benchmark — FP 混淆型安全样本（CWE-89, 难度 L3）
 *
 * 样本 1：泛型/包装数值类型 source 实为安全
 *   source 是 List<Long> / Optional<Long>（数字类型），即便拼入 Statement，
 *   也不可能构成 SQL 字符串注入——数值类型无注入语法。对照真 bx 为 List<String>。
 * 安全底线：所有 Payload 仅 localhost 演示语义，不写真实利用脚本。
 */
package blinded;

import java.sql.Statement;
import java.util.List;
import java.util.Optional;

public class NumericTypeSqlBy {

    




    static void by(Statement stmt, List<Long> ids) throws Exception {
        StringBuilder in = new StringBuilder();
        for (Long id : ids) {
            in.append(id).append(",");
        }
        String sql = "SELECT * FROM users WHERE id IN (" + in + "0)";
        /*ANCHOR_1*/
        stmt.execute(sql);
    }

    


    static void byOptional(Statement stmt, Optional<Long> optId) throws Exception {
        if (optId.isPresent()) {
            String sql = "SELECT * FROM users WHERE id = " + optId.get();
            /*ANCHOR_2*/
            stmt.execute(sql);
        }
    }
}
