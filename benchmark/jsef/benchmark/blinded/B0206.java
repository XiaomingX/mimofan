/*
 * JSEF Benchmark 样本 — 缺失功能级访问控制安全对照 (CWE-862, L3)
 * 执行前校验角色。
 * 安全底线：仅 localhost 演示语义。
 */
package blinded;

public class MissingAuthzAdminBy {

    static String deleteUser(long targetId, String currentRole) {
        if (!"ADMIN".equals(currentRole)) { // 角色校验
            throw new SecurityException("forbidden");
        }
        /*ANCHOR_1*/
        return "deleted:" + targetId;
    }
}
