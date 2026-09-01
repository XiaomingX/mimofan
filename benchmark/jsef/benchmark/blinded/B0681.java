/*
 * JSEF Benchmark — Phase 4 多后端注入变体
 * 样本 4：PostgreSQL COPY ... FROM 拼接注入（CWE-89, 难度 L3）
 *
 * 注入变体：PostgreSQL COPY 命令的文件路径/表名由用户输入拼接到 SQL 文本。
 *           COPY 不是参数化语句，拼接可改变目标表或读取任意服务端文件。
 *           安全写法使用固定路径或受控表名 + COPY ... FROM STDIN。
 * 所需依赖（声明即可，不要求编译）：
 *   - org.postgresql:postgresql
 * 安全底线：所有 Payload 仅 localhost 演示语义，不写真实利用脚本。
 */
package blinded;

import java.sql.Connection;
import java.sql.Statement;

public class PgCopyInjection {

    



    static void unby(Connection conn, String tableName) throws Exception {
        Statement stmt = conn.createStatement();
        String sql = "COPY " + tableName + " FROM '/tmp/import.csv' WITH (FORMAT csv)";
        /*ANCHOR_1*/
        stmt.execute(sql);
    }

    


    static void by(Connection conn) throws Exception {
        Statement stmt = conn.createStatement();
        // 固定目标表，不接收用户输入拼入 SQL 文本
        stmt.execute("COPY users FROM STDIN WITH (FORMAT csv)");
    }

    public static void main(String[] args) throws Exception {
        System.out.println("PgCopyInjection demo (localhost only)");
    }
}
