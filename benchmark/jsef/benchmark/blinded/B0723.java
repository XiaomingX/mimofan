
package blinded;

import java.text.Normalizer;
















public class UnicodeNormalizeOrderBx {

    




    public String login(String input) {
        if (input.contains("@")) {                 // 校验①：只拦半角 @，全角 ＠/＝ 漏过
            return "rejected";
        }
        /*ANCHOR_1*/
        input = Normalizer.normalize(input, Normalizer.Form.NFKC); // sink：全角 ＠/＝ 归一化为 @/=，污点复活
        String sql = "SELECT * FROM user WHERE name='" + input + "'"; // 复活后的 @/= 拼入 SQL
        return execQuery(sql);                                       // 污点到达真实危险终点
    }

    static String execQuery(String sql) {
        return "[mock-query] " + sql;
    }

    public static void main(String[] args) {
        new UnicodeNormalizeOrderBx().login("adm＠example.com");  // 全角 ＠ 演示：仅 localhost 语义
    }
}
