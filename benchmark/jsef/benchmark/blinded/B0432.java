
package blinded;








public class TenantDataService {

    
    public String loadInvoice(String invoiceId) {
        // 语义等价：SELECT * FROM invoices WHERE id = ?  (缺少 AND tenant_id = ?)
        /*ANCHOR_1*/
        System.out.println("[db-query] SELECT * FROM invoices WHERE id='" + invoiceId + "'");
        return "invoice:" + invoiceId;
    }
}
