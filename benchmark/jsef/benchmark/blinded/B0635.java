package blinded;

import java.util.List;
import java.util.Map;

// 仅语义模拟：JdbcTemplate 为 Spring JDBC 组件，benchmark 样本不要求编译。
// import org.springframework.jdbc.core.JdbcTemplate;























public class SlowSqlNoLimit_L2 {

    private Object jdbcTemplate;

    





    @SuppressWarnings("unchecked")
    public List<Map<String, Object>> search(String status, String keyword) {
        StringBuilder whereClause = new StringBuilder(" WHERE 1=1");
        whereClause.append(" AND status = '").append(status).append("'");
        whereClause.append(" AND note LIKE '%").append(keyword).append("%'");
        String sql = "SELECT * FROM orders" + whereClause.toString();
        /*ANCHOR_1*/
        return (List<Map<String, Object>>) queryForList(sql);
    }

    private Object queryForList(String sql) {
        return java.util.Collections.emptyList();
    }

    public static void main(String[] args) {
        new SlowSqlNoLimit_L2().search("active", "localhost-demo");
    }
}
