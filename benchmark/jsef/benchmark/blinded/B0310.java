package blinded;

import java.util.HashMap;
import java.util.Map;

/*
 * JSEF-Benchmark L4 — StoredTaintSql 安全对照（BX 混淆样本）
 *
 * 安全做法：从存储读出的值同样视为不可信，使用参数化查询（占位符），
 * 不把存储值拼入 SQL 文本。用于计算 TN / FP。
 *
 * CWE-89 (SQL Injection)。
 */
public class StoredTaintSqlBy {

    static final Map<String, String> STORE = new HashMap<>();

    static String loadFromStore(String key) {
        return STORE.getOrDefault(key, "default");
    }

    public void queryByStoredValue(String key) {
        String stored = loadFromStore(key); // 仍视为不可信
        /*ANCHOR_1*/
        jdbcTemplateQueryParam(stored); // 参数化：存储值仅作绑定参数
    }

    // 抽象 sink（安全）：语义等价 jdbcTemplate.queryForList(sql, param)
    static void jdbcTemplateQueryParam(String param) {
        System.out.println("[sql-exec-by] param=" + param);
    }

    public static void main(String[] args) {
        STORE.put("nick", "' OR '1'='1");
        new StoredTaintSqlBy().queryByStoredValue("nick");
    }
}
