
package blinded;




public class TenantContextBy {

    private static final ThreadLocal<String> CURRENT = new ThreadLocal<>();

    public void setTenant(String tenantId) {
        CURRENT.set(tenantId);
    }

    public String getTenant() {
        return CURRENT.get();
    }
}
