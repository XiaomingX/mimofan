package blinded;

import java.util.List;
import java.util.Map;

// 仅语义模拟：JdbcTemplate 为 Spring JDBC 组件，Mapper 为 MyBatis 风格接口，
// benchmark 样本不要求编译。
// import org.springframework.jdbc.core.JdbcTemplate;



























public class SlowSqlNoLimit_L3 {

    private final OrderMapper mapper = new OrderMapper();

    




    @SuppressWarnings("unchecked")
    public List<Map<String, Object>> searchByUser(String userId) {
        String fragment = " user_id = '" + userId + "'";
        /*ANCHOR_1*/
        return (List<Map<String, Object>>) mapper.queryByFragment(fragment);
    }

    // ---- Mapper 语义（同文件模拟跨编译单元）----
    static class OrderMapper {
        private Object jdbcTemplate;

        




        @SuppressWarnings("unchecked")
        Object queryByFragment(String fragment) {
            String sql = "SELECT * FROM orders WHERE " + fragment;
            /*ANCHOR_2*/
            return (List<Map<String, Object>>) queryForList(sql);
        }

        private Object queryForList(String sql) {
            return java.util.Collections.emptyList();
        }
    }

    public static void main(String[] args) {
        new SlowSqlNoLimit_L3().searchByUser("demo-user-localhost");
    }
}
