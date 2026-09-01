package blinded;

import java.util.List;
import java.util.Map;

// 仅语义模拟：JdbcTemplate 为 Spring JDBC 组件，benchmark 样本不要求编译。
// import org.springframework.jdbc.core.JdbcTemplate;













public class SlowSqlNoLimit_By {

    private Object jdbcTemplate;

    






    @SuppressWarnings("unchecked")
    public List<Map<String, Object>> searchByKeyword(String keyword, int limit, int offset) {
        String sql = "SELECT * FROM orders WHERE note LIKE ? LIMIT ? OFFSET ?";
        /*ANCHOR_1*/
        return (List<Map<String, Object>>) queryForList(sql, keyword, limit, offset);
    }

    private Object queryForList(String sql, Object... args) {
        return java.util.Collections.emptyList();
    }

    public static void main(String[] args) {
        new SlowSqlNoLimit_By().searchByKeyword("localhost-demo", 20, 0);
    }
}
