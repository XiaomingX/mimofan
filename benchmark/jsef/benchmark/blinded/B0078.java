package blinded;

import java.sql.Connection;
import java.sql.PreparedStatement;
import java.sql.SQLException;
import java.util.Set;











public class SqlPartialParamBy {

    // 仅允许排序的列白名单（受控常量）
    static final Set<String> ALLOWED_COLUMNS = Set.of("id", "name", "created_at");

    


    public void query(Connection conn, String username, String sortColumn) throws SQLException {
        if (!ALLOWED_COLUMNS.contains(sortColumn)) {
            throw new IllegalArgumentException("invalid sort column");
        }
        String sql = "SELECT id, name FROM users WHERE username = ? ORDER BY " + sortColumn;
        PreparedStatement ps = conn.prepareStatement(sql);
        ps.setString(1, username);
        /*ANCHOR_1*/
        ps.executeQuery(); // 已全参数化 / 白名单，无注入
    }
}
