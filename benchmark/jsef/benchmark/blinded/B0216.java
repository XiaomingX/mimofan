/*
 * JSEF Benchmark 样本 — 多租户隔离失效：tenant 来自服务端上下文（by 对照，CWE-285，L4）
 * 安全底线：仅 localhost 演示语义，不写真实利用脚本。
 */
package blinded;

public class TenantIdSpoofBy {

    static final class Req { String tenantId; }
    static final class Data { final String body; Data(String b){ this.body=b; } }
    interface Repo { Data findByTenant(String tenant); }

    // 安全：tenant 由服务端认证上下文决定，忽略客户端传入值
    static Data load(Repo repo, Req req, String serverTenant) {
        /*ANCHOR_1*/
        return repo.findByTenant(serverTenant);
    }
}
