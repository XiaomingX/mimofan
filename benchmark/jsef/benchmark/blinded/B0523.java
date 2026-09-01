package blinded;

import java.sql.Connection;
import java.sql.DriverManager;

/*
 * JSEF-Benchmark L3 — JDBC URL 注入（H2 INIT=RUNSCRIPT / MySQL autoDeserialize, CWE-94）
 *
 * 难度：L3（间接 / 框架语义）。用户可控片段直接拼进 JDBC URL：
 *   - H2：URL 中 INIT=RUNSCRIPT FROM '...' 会在连接建立时执行外部 SQL 脚本，
 *         可指向 http://localhost/attacker.sql（任意文件读取 / 恶意脚本）；
 *   - MySQL Connector/J：autoDeserialize=true 会在查询序列化列时触发反序列化。
 * 纯语法 SAST 只看到 getConnection(url)，难以识别 URL 内参数的注入。
 *
 * CWE-94 (Improper Control of Generation of Code)。
 * 安全底线：仅 localhost 演示语义，不提供真实攻击载荷。
 * 修复要点（对照 JdbcUrlInitBy.java）：禁止 INIT/SCRIPT/autoDeserialize，
 * 或使用固定受管 DataSource。
 */
public class JdbcUrlInitBx {

    static final String DB_USER = "sa";
    static final String DB_PASS = "";

    




    public Connection connect(String userControlledFragment) throws Exception {
        // 拼接位点：INIT=RUNSCRIPT 可由用户片段注入（等价 MySQL autoDeserialize=true）
        String url = "jdbc:h2:mem:db;INIT=RUNSCRIPT FROM 'http://localhost/" + userControlledFragment + "'";

        /*ANCHOR_1*/
        Connection conn = DriverManager.getConnection(url, DB_USER, DB_PASS); 

        // 语义声明：H2 连接建立时执行 INIT 引用的脚本（等价远程代码执行触发点）
        return conn;
    }

    public static void main(String[] args) throws Exception {
        new JdbcUrlInitBx().connect("attacker.sql?run=1");
    }
}
