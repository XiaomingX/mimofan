
package blinded;








public class TenantContext {

    private static final ThreadLocal<String> CURRENT = new ThreadLocal<>();

    public void setTenant(String tenantId) {
        // 注入当前租户（隔离边界建立点）
        /*ANCHOR_1*/
        CURRENT.set(tenantId);
    }

    public String getTenant() {
        return CURRENT.get();
    }
}
