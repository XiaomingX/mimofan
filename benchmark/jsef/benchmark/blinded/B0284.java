/*
 * JSEF Benchmark — Phase 4 多后端注入变体（安全对照）
 * 样本 3-S：JPA createNativeQuery ? 参数（CWE-89, 难度 L3）
 *
 * 与 JpaNativeQueryInjection 配对：使用 ? 位置参数绑定，用户输入不进入 SQL
 * 文本，故 expect=BX。
 * 安全底线：所有 Payload 仅 localhost 演示语义，不写真实利用脚本。
 */
package blinded;

import java.util.List;

public class JpaNativeQueryInjectionBy {

    static class Em {
        java.util.List<?> createNativeQuery(String sql, Object... params) { return java.util.List.of(); }
    }

    


    static List<?> by(String role) {
        Em em = new Em();
        /*ANCHOR_1*/
        return em.createNativeQuery("SELECT * FROM users WHERE role = ?", role);
    }

    public static void main(String[] args) {
        System.out.println("JpaNativeQueryInjectionBy demo (localhost only)");
    }
}
