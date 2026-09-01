/*
 * JSEF Benchmark — FP 混淆型安全样本的对照 bx（CWE-89, 难度 L3）
 *
 * 对照样本：MyBatis @Select 内使用 ${userName} 字符串拼接，污点到达 SQL 文本。
 * 与 MybatisHashParamBy 的 #{userName} 参数化形成 bx/by 对。
 * 安全底线：所有 Payload 仅 localhost 演示语义，不写真实利用脚本。
 */
package blinded;

import java.util.List;
import java.util.Map;

public class MybatisDollarParamInjection {

    public interface UserMapper {
        List<Map<String, Object>> findBy(String name);
    }

    



    static List<Map<String, Object>> unby(UserMapper mapper, String userName) {
        // 等价于 @Select("SELECT * FROM users WHERE name = '${userName}'")
        String sql = "SELECT * FROM users WHERE name = '" + userName + "'";
        /*ANCHOR_1*/
        return mapper.findBy(userName);
    }
}
