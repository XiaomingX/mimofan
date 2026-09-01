package blinded;

/*
 * JSEF-Benchmark L3 — 跨类静态字段安全对照
 *
 * 修复：读取静态字段后使用参数化查询（占位符 + 绑定参数）。
 * BX 侧按实现判定安全。
 */
public class StaticTaintBy {

    public static class Sink {
        static void query(String t) {
            /*ANCHOR_1*/
            jdbcTemplateParam(t);
        }
    }

    // 抽象 sink：语义等价 jdbcTemplate.query("... where x = ?", param)
    static void jdbcTemplateParam(String param) {
        System.out.println("[sql-param] " + param);
    }

    public static void main(String[] args) {
        T = "1 OR 1=1";
        Sink.query(T);
    }

    public static String T;
}
