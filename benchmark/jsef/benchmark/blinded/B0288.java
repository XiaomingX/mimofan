/*
 * JSEF Benchmark — Phase 4 多后端注入变体（安全对照）
 * 样本 4-S：PostgreSQL COPY 固定表名（CWE-89, 难度 L3）
 *
 * 与 PgCopyInjection 配对：COPY 目标表固定，不接收用户输入拼入 SQL 文本，
 * 数据经 STDIN 流式导入，故 expect=BX。
 * 安全底线：所有 Payload 仅 localhost 演示语义，不写真实利用脚本。
 */
package blinded;

import java.sql.Connection;
import java.sql.Statement;

public class PgCopyInjectionBy {

    


    static void by(Connection conn) throws Exception {
        Statement stmt = conn.createStatement();
        /*ANCHOR_1*/
        stmt.execute("COPY users FROM STDIN WITH (FORMAT csv)");
    }

    public static void main(String[] args) throws Exception {
        System.out.println("PgCopyInjectionBy demo (localhost only)");
    }
}
