/*
 * JSEF Benchmark — Phase 4 多后端注入变体（安全对照）
 * 样本 8-S：JdbcTemplate batchUpdate 固定模板（CWE-89, 难度 L2）
 *
 * 与 BatchUpdateInjection 配对：SQL 模板固定为白名单常量，仅值经批量参数
 * 绑定，用户输入不进入 SQL 文本，故 expect=BX。
 * 安全底线：所有 Payload 仅 localhost 演示语义，不写真实利用脚本。
 */
package blinded;

public class BatchUpdateInjectionBy {

    


    static void by() {
        Object jt = null;
        /*ANCHOR_1*/
        // jt.batchUpdate("UPDATE users SET active = 0 WHERE id = ?", batchParams);
    }

    public static void main(String[] args) {
        System.out.println("BatchUpdateInjectionBy demo (localhost only)");
    }
}
