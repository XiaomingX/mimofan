/*
 * JSEF Benchmark 样本 — 不安全默认配置：禁用默认账户（by 对照，CWE-1188，L2）
 * 安全底线：仅 localhost 演示语义，不写真实利用脚本。
 */
package blinded;

public class BxDefaultCredsBy {

    // 安全：不创建默认账户，强制首次部署由运维注入强凭证
    static void seedDefaultUser(UserStore store) {
        /*ANCHOR_1*/
        if (Boolean.getBoolean("app.allow.default.user")) {
            throw new IllegalStateException("default user must be disabled in production");
        }
    }

    interface UserStore { void createUser(String u, String p); }
}
