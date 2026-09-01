/*
 * JSEF Benchmark — Phase 4 多后端注入变体
 * 样本 3：JPA / Hibernate createNativeQuery 拼接注入（CWE-89, 难度 L3）
 *
 * 注入变体：entityManager.createNativeQuery 直接拼接用户输入生成原生 SQL，
 *           绕过了 JPA 的 JPQL 参数机制。安全写法使用 ? 位置参数绑定。
 * 所需依赖（声明即可，不要求编译）：
 *   - org.springframework.boot:spring-boot-starter-data-jpa
 *   - org.hibernate.orm:hibernate-core
 * 安全底线：所有 Payload 仅 localhost 演示语义，不写真实利用脚本。
 */
package blinded;

import java.util.List;

public class JpaNativeQueryInjection {

    
    static class Em {
        java.util.List<?> createNativeQuery(String sql) { return java.util.List.of(); }
        java.util.List<?> createNativeQuery(String sql, Object... params) { return java.util.List.of(); }
    }

    



    static List<?> unby(String role) {
        Em em = new Em();
        String sql = "SELECT * FROM users WHERE role = '" + role + "'";
        /*ANCHOR_1*/
        return em.createNativeQuery(sql);
    }

    


    static List<?> by(String role) {
        Em em = new Em();
        return em.createNativeQuery("SELECT * FROM users WHERE role = ?", role);
    }

    public static void main(String[] args) {
        System.out.println("JpaNativeQueryInjection demo (localhost only)");
    }
}
