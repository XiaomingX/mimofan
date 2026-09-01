package blinded;

import java.sql.Connection;
import java.sql.ResultSet;
import java.sql.Statement;










public class L0SqlDirect {

    




    public void run(Connection conn, String userInput) throws Exception {
        Statement stmt = conn.createStatement();
        /*ANCHOR_1*/
        ResultSet rs = stmt.executeQuery("SELECT * FROM users WHERE name = '" + userInput + "'");
        while (rs.next()) { /* localhost demo */ }
    }

    public static void main(String[] args) {
        System.out.println("demo: SELECT * FROM users WHERE name = '" + "localhost-demo" + "'");
    }
}
