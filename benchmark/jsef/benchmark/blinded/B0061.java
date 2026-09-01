
package blinded;

import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.RequestHeader;
import org.springframework.web.bind.annotation.RequestParam;
import org.springframework.web.bind.annotation.RestController;







@RestController
public class TenantInvoiceControllerBy {

    private final TenantContextBy tenantContext;
    private final TenantDataServiceBy dataService;

    public TenantInvoiceControllerBy(TenantContextBy tenantContext,
                                       TenantDataServiceBy dataService) {
        this.tenantContext = tenantContext;
        this.dataService = dataService;
    }

    @GetMapping("/api/v1/invoices")
    public String getInvoice(@RequestHeader("X-Tenant-Id") String tenantId,
                             @RequestParam("invoiceId") String invoiceId) {
        tenantContext.setTenant(tenantId);
        // 安全：查询由 DataService 内部绑定租户，越权访问被拒
        /*ANCHOR_1*/
        return dataService.loadInvoice(invoiceId);
    }
}
