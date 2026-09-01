package blinded;

import java.sql.Connection;
import java.sql.PreparedStatement;
import java.sql.ResultSet;









public class L0SqlDirectBy {

    




    public void run(Connection conn, String userInput) throws Exception {
        PreparedStatement ps = conn.prepareStatement("SELECT * FROM users WHERE name = ?");
        ps.setString(1, userInput);
        /*ANCHOR_1*/
        ResultSet rs = ps.executeQuery();
        while (rs.next()) { /* localhost demo */ }
    }

    public static void main(String[] args) {
        System.out.println("demo: parameterized query with localhost-demo");
    }
}
