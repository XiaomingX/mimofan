/*
 * JSEF Benchmark — Phase 4 多后端注入变体
 * 样本 6：NamedParameterJdbcTemplate 误用 ${} 而非 :param（CWE-89, 难度 L2）
 *
 * 注入变体：NamedParameterJdbcTemplate 本应写 ":col" 由框架参数化，但开发者
 *           错误地把列名用 ${} 直接拼入（或字符串连接），导致命名参数机制失效。
 * 所需依赖（声明即可，不要求编译）：
 *   - org.springframework.boot:spring-boot-starter-jdbc
 * 安全底线：所有 Payload 仅 localhost 演示语义，不写真实利用脚本。
 */
package blinded;

import java.util.List;
import java.util.Map;

public class JdbcNamedParamAbuse {

    



    static List<Map<String, Object>> unby(String sortColumn) {
        // 模拟 NamedParameterJdbcTemplate（语义占位）
        Object template = null;
        String sql = "SELECT * FROM users ORDER BY " + sortColumn;
        /*ANCHOR_1*/
        return java.util.List.of();
    }

    


    static List<Map<String, Object>> by(String sortColumn) {
        if (!List.of("id", "username", "email").contains(sortColumn)) {
            throw new IllegalArgumentException("invalid sort column");
        }
        return java.util.List.of();
    }

    public static void main(String[] args) {
        System.out.println("JdbcNamedParamAbuse demo (localhost only)");
    }
}
