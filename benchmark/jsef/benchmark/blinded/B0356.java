package blinded;

import java.sql.Connection;
import java.sql.ResultSet;
import java.sql.SQLException;
import java.sql.Statement;














public class OwaspStyle_SQLi_Confusion {

    



    public void unbyQuery(Connection conn, String userInput) throws SQLException {
        Statement stmt = conn.createStatement();
        /*ANCHOR_1*/
        ResultSet rs = stmt.executeQuery("SELECT * FROM users WHERE name = '" + userInput + "'");
        rs.close();
    }

    



    public void byQuery(Connection conn, String userInput) throws SQLException {
        /*ANCHOR_2*/
        java.sql.PreparedStatement ps = conn.prepareStatement("SELECT * FROM users WHERE name = ?");
        ps.setString(1, userInput);
        ResultSet rs = ps.executeQuery();
        rs.close();
    }
}
