/*
 * JSEF Benchmark 样本 — 多租户隔离失效：服务端 tenant 上下文隔离（by 对照，CWE-639，L3）
 * 安全底线：仅 localhost 演示语义，不写真实利用脚本。
 */
package blinded;

public class TenantDataLeakBy {

    static final class Record { final String id; final String tenantId; Record(String id, String t){ this.id=id; this.tenantId=t; } }
    interface Repo { Record findByIdAndTenant(String id, String tenant); }

    // 安全：查询强制带上服务端 tenant 上下文
    static Record get(Repo repo, String id, String serverTenant) {
        /*ANCHOR_1*/
        Record r = repo.findByIdAndTenant(id, serverTenant);
        if (r == null) throw new SecurityException("record not found for tenant");
        return r;
    }
}
