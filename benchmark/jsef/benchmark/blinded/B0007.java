/*
 * JSEF Benchmark 真假混淆样本 — SQL 白名单（B5，CWE-89）
 * BX 版：输入经严格白名单校验后才拼（白名单内容安全），看似拼接实则受控。
 * 文件头说明：独立 benchmark 源文件，使用模拟 jdbcTemplate 表达 sink，不强求编译。
 */
public class ConfusionSqlWhitelistBy {

    static java.util.List<?> query(String sql) {
        return java.util.List.of(); // SINK（语义）
    }

    // 严格白名单：仅允许已知集合中的列名（内容本身安全）
    static final java.util.Set<String> ALLOWED = java.util.Set.of("name", "email", "role");

    


    static java.util.List<?> by(String userInput) {
        /*ANCHOR_1*/
        if (!ALLOWED.contains(userInput)) {
            throw new IllegalArgumentException("invalid column");
        }
        // 拼入的是白名单常量列名，用户输入已被拒绝进入 SQL，无注入
        String sql = "SELECT * FROM users ORDER BY " + userInput;
        return query(sql);
    }
}
