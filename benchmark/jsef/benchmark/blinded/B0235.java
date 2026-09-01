package blinded;

import java.sql.Connection;
import java.sql.ResultSet;
import java.sql.Statement;

// 仅语义模拟：java.sql.* 为 JDK 标准 JDBC API，benchmark 样本不要求编译。
// import javax.sql.DataSource;













public class DbResourceLeak_By {

    private Object dataSource; // 语义模拟 DataSource

    


    @SuppressWarnings("unchecked")
    public void query() throws Exception {
        try (Connection conn = getConnection();
             Statement stmt = conn.createStatement();
             ResultSet rs = stmt.executeQuery("SELECT * FROM orders LIMIT 100")) {
            while (rs.next()) {
                // 处理行...（演示省略）
            }
            /*ANCHOR_1*/
        }
    }

    private Connection getConnection() throws Exception {
        return null; // 语义占位
    }

    public static void main(String[] args) throws Exception {
        new DbResourceLeak_By().query();
    }
}
