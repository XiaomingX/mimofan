package blinded;

import java.util.List;
import java.util.Map;

// 仅语义模拟：JdbcTemplate 为 Spring JDBC 组件，benchmark 样本不要求编译。
// import org.springframework.jdbc.core.JdbcTemplate;























public class SlowSqlNoLimit_L1 {

    // 语义模拟的模板（不可信源），真实场景来自 Spring 注入的 JdbcTemplate
    private Object jdbcTemplate;

    




    @SuppressWarnings("unchecked")
    public List<Map<String, Object>> searchByKeyword(String keyword) {
        String sql = "SELECT * FROM orders WHERE note LIKE '%" + keyword + "%'";
        /*ANCHOR_1*/
        return (List<Map<String, Object>>) queryForList(sql);
    }

    // 语义占位：模拟 JdbcTemplate.queryForList
    private Object queryForList(String sql) {
        return java.util.Collections.emptyList();
    }

    public static void main(String[] args) {
        new SlowSqlNoLimit_L1().searchByKeyword("localhost-demo");
    }
}
