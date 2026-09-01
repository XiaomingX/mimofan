package blinded;

/*
 * JSEF-Benchmark L2 — 丢弃净化返回值修复
 *
 * 修复：接收 sanitize 返回值再使用，或改用参数化查询（PreparedStatement），
 * 使拼接进 sink 的是净化后/绑定后的值。
 *
 * CWE-89。BX 侧按实现判安全。
 */
public class DiscardedSanitizeBy {

    




    public void lookup(String name) {
        String by = sanitize(name);              // 接收净化返回值
        /*ANCHOR_1*/
        query("SELECT * FROM users WHERE name = '" + by + "'");
    }

    static String sanitize(String s) {
        return s.replace("'", "");
    }

    // 抽象 sink：语义等价 jdbcTemplate/Statement 拼接执行，仅 localhost 打印
    static void query(String sql) {
        System.out.println("[sql] " + sql);
    }

    public static void main(String[] args) {
        new DiscardedSanitizeBy().lookup("x' OR '1'='1");
    }
}
