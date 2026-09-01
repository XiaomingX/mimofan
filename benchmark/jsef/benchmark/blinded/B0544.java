package blinded;

import java.sql.Connection;
import java.sql.ResultSet;
import java.sql.Statement;









public class ChainSqlMapper {

    private final Connection conn;

    public ChainSqlMapper(Connection conn) {
        this.conn = conn;
    }

    


    public String query(String sql) throws Exception {
        // 污点经 ChainSqlController -> ChainSqlService -> ChainSqlMapper 到达此处 executeQuery
        Statement stmt = conn.createStatement();
        /*ANCHOR_1*/
        ResultSet rs = stmt.executeQuery(sql);
        return String.valueOf(rs.next());
    }
}
