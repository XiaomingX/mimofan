package blinded;

import java.sql.Connection;
import java.sql.PreparedStatement;
import java.sql.SQLException;















public class SqlPartialParam {

    


    public void query(Connection conn, String username, String sortColumn) throws SQLException {
        // 仅首参数参数化 —— 看似安全
        String sql = "SELECT id, name FROM users WHERE username = ? ORDER BY " + sortColumn;
        PreparedStatement ps = conn.prepareStatement(sql);
        ps.setString(1, username); // 占位符：正确
        /*ANCHOR_1*/
        ps.executeQuery(); // sortColumn 直连 sink，注入仍存在
    }
}
