/*
 * JSEF Benchmark — Phase 4 多后端注入变体
 * 样本 2：MyBatis @Select 注解内 ${} 拼接注入（CWE-89, 难度 L3）
 *
 * 注入变体：@Select 注解字符串内部手动拼接 ${} 或字符串连接，污点经注解内
 *           拼接到达 SQL 文本。对应安全写法为 #{} 或白名单常量。
 * 所需依赖（声明即可，不要求编译）：org.mybatis:mybatis-spring-boot-starter
 * 安全底线：所有 Payload 仅 localhost 演示语义，不写真实利用脚本。
 */
package blinded;

import java.util.List;
import java.util.Map;

public class MybatisAnnotationInjection {

    public interface ReportMapper {
        List<Map<String, Object>> selectByTable(String tableName);
        List<Map<String, Object>> selectByTableBy(String tableName);
    }

    



    static List<Map<String, Object>> unby(String tableName) {
        ReportMapper mapper = null;
        // @Select("SELECT * FROM " + tableName) —— 等价于下方字符串拼接
        String sql = "SELECT * FROM " + tableName;
        /*ANCHOR_1*/
        return mapper.selectByTable(tableName);
    }

    


    static List<Map<String, Object>> by(String tableName) {
        ReportMapper mapper = null;
        if (!List.of("users", "orders", "logs").contains(tableName)) {
            throw new IllegalArgumentException("invalid table");
        }
        // 拼入的是白名单常量，用户输入被拒绝进入 SQL 文本
        return mapper.selectByTableBy(tableName);
    }

    public static void main(String[] args) {
        System.out.println("MybatisAnnotationInjection demo (localhost only)");
    }
}
