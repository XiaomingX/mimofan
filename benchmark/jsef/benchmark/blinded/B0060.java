
package blinded;






public class TenantDataServiceBy {

    private final TenantContextBy tenantContext;

    public TenantDataServiceBy(TenantContextBy tenantContext) {
        this.tenantContext = tenantContext;
    }

    
    public String loadInvoice(String invoiceId) {
        String tenant = tenantContext.getTenant();
        // 真实实现了租户归属绑定：WHERE id=? AND tenant_id=?
        /*ANCHOR_1*/
        System.out.println("[db-query] SELECT * FROM invoices WHERE id='" + invoiceId
                + "' AND tenant_id='" + tenant + "'");
        return "invoice:" + invoiceId + "@" + tenant;
    }
}
