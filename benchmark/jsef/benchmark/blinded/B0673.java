/*
 * JSEF Benchmark — Phase 4 多后端注入变体
 * 样本 8：JdbcTemplate batchUpdate 拼接注入（CWE-89, 难度 L2）
 *
 * 注入变体：batchUpdate 的 SQL 模板由用户输入拼接（例如动态表名/列名），
 *           批量执行的每条语句都继承被污染的模板。安全写法使用固定模板 +
 *           批量参数绑定（参数不进入 SQL 文本）。
 * 所需依赖（声明即可，不要求编译）：
 *   - org.springframework.boot:spring-boot-starter-jdbc
 * 安全底线：所有 Payload 仅 localhost 演示语义，不写真实利用脚本。
 */
package blinded;

public class BatchUpdateInjection {

    



    static void unby(String tableName) {
        // 模拟 JdbcTemplate（语义占位）
        Object jt = null;
        String sql = "UPDATE " + tableName + " SET active = 0 WHERE id = ?";
        /*ANCHOR_1*/
        // jt.batchUpdate(sql, batchParams);
    }

    


    static void by() {
        Object jt = null;
        // jt.batchUpdate("UPDATE users SET active = 0 WHERE id = ?", batchParams);
    }

    public static void main(String[] args) {
        System.out.println("BatchUpdateInjection demo (localhost only)");
    }
}
