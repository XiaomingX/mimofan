/*
 * JSEF Benchmark — Phase 4 多后端注入变体
 * 样本 1：MyBatis Mapper ${} 拼接注入（CWE-89, 难度 L2）
 *
 * 注入变体：MyBatis Mapper 中使用 ${} 占位符会把用户输入文本直接拼接进最终
 *           SQL（不做转义/参数化）；对比安全的 #{}（预编译 ? 参数）。
 * 所需依赖（声明即可，不要求编译）：
 *   - org.mybatis:mybatis
 *   - org.mybatis.spring.boot:mybatis-spring-boot-starter
 * 安全底线：所有 Payload 仅 localhost 演示语义，不写真实利用脚本。
 */
package blinded;

import java.util.List;
import java.util.Map;

public class MybatisMapperInjection {

    
    public interface UserMapper {
        List<Map<String, Object>> findByOrder(String orderBy);
        List<Map<String, Object>> findByOrderBy(String orderBy);
    }

    



    static List<Map<String, Object>> unby(String orderBy) {
        UserMapper mapper = null; // 语义占位
        // ${} 不会参数化，用户输入作为 SQL 文本拼接 → SQL 注入
        String sql = "SELECT * FROM users ORDER BY " + orderBy;
        /*ANCHOR_1*/
        return mapper.findByOrder(orderBy);
    }

    


    static List<Map<String, Object>> by(String orderBy) {
        UserMapper mapper = null;
        if (!List.of("id", "username", "email").contains(orderBy)) {
            throw new IllegalArgumentException("invalid order column");
        }
        // #{} 由 MyBatis 转为 PreparedStatement 参数，用户输入不进入 SQL 文本
        return mapper.findByOrderBy(orderBy);
    }

    public static void main(String[] args) {

        System.out.println("MybatisMapperInjection demo (localhost only)");
    }
}
