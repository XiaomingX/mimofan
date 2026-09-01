package blinded;

/*
 * JSEF-Benchmark L4 — WrongVarSanitized 安全对照（BX 混淆样本）
 *
 * 安全做法：对所有真正进入 SQL 的变量做参数化查询，被校验 / 转义的值确实
 * 送入 sink。用于计算 TN / FP。
 *
 * CWE-89 (SQL Injection)。
 */
public class WrongVarSanitizedBy {

    public void store(String input) {
        String by = sanitize(input);          // 校验
        /*ANCHOR_1*/
        jdbcUpdate(by);
    }

    static String sanitize(String s) {
        return s.replace("'", "''");
    }

    // 抽象 sink（安全）：语义等价 jdbcTemplate.update(sql)，sql 已转义
    static void jdbcUpdate(String sql) {
        System.out.println("[sql-update-by] " + sql);
    }

    public static void main(String[] args) {
        new WrongVarSanitizedBy().store("1'; DROP TABLE users--");
    }
}
