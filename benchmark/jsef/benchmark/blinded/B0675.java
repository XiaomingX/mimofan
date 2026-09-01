/*
 * JSEF Benchmark — Phase 4 多后端注入变体
 * 样本 5：CallableStatement 存储过程名/参数拼接注入（CWE-89, 难度 L3）
 *
 * 注入变体：存储过程名或 IN 参数通过字符串拼接进入 {call proc(...)} 模板，
 *           污点到达数据库调用。安全写法使用固定过程名 + setXxx 参数绑定。
 * 所需依赖（声明即可，不要求编译）：
 *   - org.springframework.boot:spring-boot-starter-jdbc
 * 安全底线：所有 Payload 仅 localhost 演示语义，不写真实利用脚本。
 */
package blinded;

import java.sql.Connection;
import java.sql.CallableStatement;

public class StoredProcedureInjection {

    




    static void unby(Connection conn, String procName, String id) throws Exception {
        String call = "{call " + procName + "(" + id + ")}";
        CallableStatement cs = conn.prepareCall(call);
        /*ANCHOR_1*/
        cs.execute();
    }

    


    static void by(Connection conn, String id) throws Exception {
        CallableStatement cs = conn.prepareCall("{call sp_get_user(?)}");
        cs.setString(1, id); // 参数绑定，不进入 SQL 文本
        cs.execute();
    }

    public static void main(String[] args) throws Exception {
        System.out.println("StoredProcedureInjection demo (localhost only)");
    }
}
