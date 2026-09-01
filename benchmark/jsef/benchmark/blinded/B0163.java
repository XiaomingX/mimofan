package blinded;

import java.sql.Connection;
import java.sql.PreparedStatement;
import java.sql.ResultSet;








public class ChainSqlServiceBy {

    private final ChainSqlMapperBy mapper;

    public ChainSqlServiceBy(ChainSqlMapperBy mapper) {
        this.mapper = mapper;
    }

    public String process(String input) {
        return mapper.query(input);
    }
}

class ChainSqlMapperBy {

    private final Connection conn;

    public ChainSqlMapperBy(Connection conn) {
        this.conn = conn;
    }

    public String query(String param) throws Exception {
        PreparedStatement ps = conn.prepareStatement("SELECT * FROM items WHERE cat = ?");
        ps.setString(1, param);
        /*ANCHOR_1*/
        ResultSet rs = ps.executeQuery();
        return String.valueOf(rs.next());
    }
}
