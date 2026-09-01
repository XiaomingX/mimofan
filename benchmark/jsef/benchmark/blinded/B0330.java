package blinded;

import java.text.Normalizer;










public class UnicodeNormalizeOrderBy {

    




    public String login(String input) {
        input = Normalizer.normalize(input, Normalizer.Form.NFKC); // 先归一化：全角 ＠/＝ → @/=
        if (input.contains("@")) {                                // 再校验：拦截归一化后的 @
            return "rejected";
        }
        /*ANCHOR_1*/
        String sql = "SELECT * FROM user WHERE name='" + input + "'"; // 已过滤，安全
        return execQuery(sql);
    }

    static String execQuery(String sql) {
        return "[mock-query] " + sql;
    }

    public static void main(String[] args) {
        new UnicodeNormalizeOrderBy().login("adm＠example.com");
    }
}
