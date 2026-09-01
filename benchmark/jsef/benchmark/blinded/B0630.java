package blinded;

import java.sql.Connection;
import java.sql.ResultSet;
import java.sql.Statement;

// 仅语义模拟：java.sql.* 为 JDK 标准 JDBC API，benchmark 样本不要求编译。
// import javax.sql.DataSource;























public class DbResourceLeak {

    private Object dataSource; // 语义模拟 DataSource

    


    @SuppressWarnings("unchecked")
    public void query() throws Exception {
        Connection conn = getConnection(); // 语义模拟：从池获取
        Statement stmt = conn.createStatement();
        ResultSet rs = stmt.executeQuery("SELECT * FROM orders");
        while (rs.next()) {
            // 处理行...（演示省略）
        }
        /*ANCHOR_1*/
        // 缺陷：conn/stmt/rs 均未关闭，异常时直接泄漏，连接池耗尽导致 DoS
    }

    private Connection getConnection() throws Exception {
        return null; // 语义占位
    }

    public static void main(String[] args) throws Exception {
        new DbResourceLeak().query();
    }
}
