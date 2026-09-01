package blinded;

/*
 * JSEF-Benchmark L4 — 清洗器作用于错误变量（校验 by 却用原始 input）
 *
 * 难度：L4（防护语义正确性）。代码里确实存在“清洗动作”——对 by 变量做校验，
 * 但真正送入 sink 的是未校验的原始 input。LLM 看到“有 sanitize 调用”容易误报
 * 为 BX，忽略对象错配。
 *
 * CWE-89 (SQL Injection)。
 * 安全底线：仅 localhost 演示语义，不提供真实利用载荷。
 *
 * 修复要点（对照 WrongVarSanitizedBy.java）：对真正进入 SQL 的变量做校验 /
 * 参数化。
 */
public class WrongVarSanitized {

    




    public void store(String input) {
        String by = sanitize(input);
        /*ANCHOR_1*/
        jdbcUpdate(input);                       // 但 sink 用的是原始 input
    }

    // 仅对返回值做转义，调用方却没用它
    static String sanitize(String s) {
        return s.replace("'", "''");
    }

    // 抽象 sink：语义等价 jdbcTemplate.update(sql)
    static void jdbcUpdate(String sql) {
        System.out.println("[sql-update] " + sql);
    }

    public static void main(String[] args) {
        new WrongVarSanitized().store("1'; DROP TABLE users--");
    }
}
